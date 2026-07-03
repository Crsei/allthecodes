//! Settings validation.
//!
//! Validates configuration values and reports warnings for
//! invalid or suspicious settings.

use anyhow::{bail, Result};
use serde_json::Value;

use crate::permission_validation::{self, PermissionValidationWarning};
use crate::runtime_settings::SettingsJson;
use crate::settings::LoadedSettings;
use crate::validation_tips::{self, ValidationTip};

const VALID_BACKENDS: &[&str] = &["native", "codex"];
const VALID_API_PROVIDERS: &[&str] = &["anthropic", "openai-codex", "openai"];

// ---------------------------------------------------------------------------
// Engine-layer constants duplicated here
// ---------------------------------------------------------------------------
//
// These were root engine output-style and effort helpers before cc-config was
// split off in Phase 3 (issue #72). Moving `engine::output_style` and
// `engine::effort` into cc-config would drag the full engine graph in;
// validation just needs the name/budget lookup, so we duplicate the
// small amount of data here. If these lists drift, either source can
// update independently — the authoritative engine values remain the
// ones used at runtime.
const BUILT_IN_STYLE_NAMES: &[&str] = &["default", "explanatory", "learning"];

type SettingsPresencePredicate = fn(&SettingsJson) -> bool;
type SettingsPresenceRule = (&'static str, SettingsPresencePredicate);

/// Accept any label the engine's effort resolver would accept.
/// Mirrors `engine::effort::effort_to_budget_tokens` — kept in sync by hand.
fn effort_label_is_known(effort: &str) -> bool {
    let trimmed = effort.trim();
    if trimmed.is_empty() {
        return false;
    }
    if let Ok(n) = trimmed.parse::<u32>() {
        return n > 0;
    }
    matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "low" | "medium" | "med" | "high" | "xhigh" | "auto" | "max"
    )
}

fn model_reasoning_effort_is_known(effort: &str) -> bool {
    matches!(
        effort.trim().to_ascii_lowercase().as_str(),
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh"
    )
}

const ELECTRON_LEGACY_SETTINGS: &[SettingsPresenceRule] = &[
    ("appIcon", |settings| settings.app_icon.is_some()),
    ("autoStart", |settings| settings.auto_start.is_some()),
    ("startMinimized", |settings| {
        settings.start_minimized.is_some()
    }),
    ("minimizeToTray", |settings| {
        settings.minimize_to_tray.is_some()
    }),
    ("closeToTray", |settings| settings.close_to_tray.is_some()),
    ("quickChatHideOnBlur", |settings| {
        settings.quick_chat_hide_on_blur.is_some()
    }),
    ("quickChatInjectScreen", |settings| {
        settings.quick_chat_inject_screen.is_some()
    }),
    ("quickChatAmbient", |settings| {
        settings.quick_chat_ambient.is_some()
    }),
];

const RESERVED_MEMORY_SETTINGS: &[SettingsPresenceRule] = &[
    ("memoryAutoRetrieve", |settings| {
        settings.memory_auto_retrieve.is_some()
    }),
    ("memoryQueryRewriting", |settings| {
        settings.memory_query_rewriting.is_some()
    }),
    ("memoryMaxRetrieved", |settings| {
        settings.memory_max_retrieved.is_some()
    }),
    ("memorySimilarityThreshold", |settings| {
        settings.memory_similarity_threshold.is_some()
    }),
    ("memoryAutoSummarize", |settings| {
        settings.memory_auto_summarize.is_some()
    }),
    ("memoryNightly", |settings| {
        settings.memory_nightly.is_some()
    }),
    ("memorySleepTime", |settings| {
        settings.memory_sleep_time.is_some()
    }),
    ("memoryTempTtl", |settings| {
        settings.memory_temp_ttl.is_some()
    }),
    ("memoryArchiveRetention", |settings| {
        settings.memory_archive_retention.is_some()
    }),
    ("memoryToolModel", |settings| {
        settings.memory_tool_model.is_some()
    }),
    ("memoryEmbeddingModel", |settings| {
        settings.memory_embedding_model.is_some()
    }),
];

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Severity level for validation warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningSeverity {
    /// Informational note, not a problem.
    Info,
    /// Something looks off but may still work.
    Warning,
    /// Invalid value that will cause failures.
    Error,
}

/// A single validation warning produced when checking settings.
#[derive(Debug, Clone)]
pub struct ValidationWarning {
    /// The setting field that triggered the warning.
    pub field: String,
    /// Human-readable description of the issue.
    pub message: String,
    /// How serious the issue is.
    pub severity: WarningSeverity,
}

// ---------------------------------------------------------------------------
// Known model patterns
// ---------------------------------------------------------------------------

/// Prefixes that are recognized as valid Anthropic model identifiers.
const VALID_MODEL_PREFIXES: &[&str] = &["claude-", "anthropic."];

/// Specific model names that are always valid (aliases, etc.).
const VALID_MODEL_NAMES: &[&str] = &[
    "claude-opus-4-6-20250414",
    "claude-sonnet-4-20250514",
    "claude-haiku-4-5",
    "claude-3-opus-20240229",
    "claude-3-sonnet-20240229",
    "claude-3-haiku-20240307",
    "claude-3-5-sonnet-20241022",
    "claude-3-5-haiku-20241022",
];

/// Model prefixes from third-party providers (OpenAI, Google, etc.)
/// that we also accept.
const THIRD_PARTY_PREFIXES: &[&str] = &[
    "gpt-",
    "o1-",
    "o3-",
    "gemini-",
    "models/gemini-",
    "deepseek-",
    "mistral-",
    "codestral-",
    "command-",
    "accounts/", // Vertex AI paths
];

// ---------------------------------------------------------------------------
// Validation functions
// ---------------------------------------------------------------------------

/// Validate a model name.
///
/// Returns `Ok(())` if the model name matches a known pattern.
/// Returns an error if the name is clearly invalid.
///
/// Note: this is a best-effort check. Unknown but syntactically valid
/// model names are accepted to allow for new model releases.
pub fn validate_model_name(model: &str) -> Result<()> {
    let trimmed = model.trim();

    if trimmed.is_empty() {
        bail!("Model name cannot be empty");
    }

    if trimmed.len() > 256 {
        bail!("Model name is too long (max 256 characters)");
    }

    // Check for obviously invalid characters
    if trimmed.contains(char::is_whitespace) {
        bail!("Model name cannot contain whitespace: '{}'", trimmed);
    }

    // Accept exact known model names
    if VALID_MODEL_NAMES.contains(&trimmed) {
        return Ok(());
    }

    // Accept known prefixes
    let lower = trimmed.to_lowercase();
    for prefix in VALID_MODEL_PREFIXES
        .iter()
        .chain(THIRD_PARTY_PREFIXES.iter())
    {
        if lower.starts_with(prefix) {
            return Ok(());
        }
    }

    // Accept model names that look like provider paths (contain a `/`)
    if trimmed.contains('/') {
        return Ok(());
    }

    bail!(
        "Unrecognized model name: '{}'. Expected a model ID like \
         'claude-sonnet-4-20250514' or 'claude-opus-4-6-20250414'.",
        trimmed
    )
}

/// Validate all settings in a `SettingsJson` and return a list of warnings.
///
/// Does not fail hard -- returns warnings that the caller can display or log.
pub fn validate_settings(settings: &SettingsJson) -> Vec<ValidationWarning> {
    let mut warnings = Vec::new();

    for (field, is_set) in ELECTRON_LEGACY_SETTINGS {
        if is_set(settings) {
            warnings.push(ValidationWarning {
                field: (*field).to_string(),
                message: format!(
                    "{field} is accepted for settings compatibility, but the Rust TUI does not support the legacy Electron behavior and this setting has no runtime effect."
                ),
                severity: WarningSeverity::Info,
            });
        }
    }

    if settings.auto_approve_tools.is_some() {
        warnings.push(ValidationWarning {
            field: "autoApproveTools".to_string(),
            message: "autoApproveTools is accepted for compatibility, but it is not wired to the Rust permission system and has no runtime effect. Use permissionMode or permissions.defaultMode instead.".to_string(),
            severity: WarningSeverity::Info,
        });
    }
    if settings.analytics_enabled.is_some() {
        warnings.push(ValidationWarning {
            field: "analyticsEnabled".to_string(),
            message: "analyticsEnabled is accepted for compatibility, but analytics collection is not implemented and this setting has no runtime effect.".to_string(),
            severity: WarningSeverity::Info,
        });
    }
    if settings.teammate_mode.is_some() {
        warnings.push(ValidationWarning {
            field: "teammateMode".to_string(),
            message: "teammateMode is accepted for compatibility, but the Rust TUI does not currently implement teammate display modes and this setting has no runtime effect.".to_string(),
            severity: WarningSeverity::Info,
        });
    }
    for (field, is_set) in RESERVED_MEMORY_SETTINGS {
        if is_set(settings) {
            warnings.push(ValidationWarning {
                field: (*field).to_string(),
                message: format!(
                    "{field} is reserved for future memory runtime tuning and currently has no runtime effect."
                ),
                severity: WarningSeverity::Info,
            });
        }
    }

    // Validate model name
    if let Some(ref model) = settings.model {
        if model.trim().is_empty() {
            warnings.push(ValidationWarning {
                field: "model".to_string(),
                message: "Model name is set but empty. The default model will be used.".to_string(),
                severity: WarningSeverity::Warning,
            });
        } else if let Err(e) = validate_model_name(model) {
            warnings.push(ValidationWarning {
                field: "model".to_string(),
                message: format!("Invalid model: {}", e),
                severity: WarningSeverity::Error,
            });
        }
    }

    for (field, model) in [
        ("defaultModel", settings.default_model.as_ref()),
        ("fallbackModel", settings.fallback_model.as_ref()),
        ("fastModel", settings.fast_model.as_ref()),
        ("sotaModel", settings.sota_model.as_ref()),
        ("motaModel", settings.mota_model.as_ref()),
        ("fotaModel", settings.fota_model.as_ref()),
    ] {
        if let Some(model) = model {
            if model.trim().is_empty() {
                warnings.push(ValidationWarning {
                    field: field.to_string(),
                    message: format!("{field} is set but empty."),
                    severity: WarningSeverity::Warning,
                });
            } else if let Err(e) = validate_model_name(model) {
                warnings.push(ValidationWarning {
                    field: field.to_string(),
                    message: format!("Invalid model: {}", e),
                    severity: WarningSeverity::Error,
                });
            }
        }
    }

    if let Some(ref backend) = settings.backend {
        let normalized = backend.trim().to_ascii_lowercase();
        if !normalized.is_empty() && !VALID_BACKENDS.contains(&normalized.as_str()) {
            warnings.push(ValidationWarning {
                field: "backend".to_string(),
                message: format!(
                    "Unknown backend '{}'. Known backends: {}.",
                    backend,
                    VALID_BACKENDS.join(", ")
                ),
                severity: WarningSeverity::Error,
            });
        }
    }

    if let Some(ref provider) = settings.api_provider {
        let normalized = provider.trim().to_ascii_lowercase().replace('_', "-");
        if !normalized.is_empty() && !VALID_API_PROVIDERS.contains(&normalized.as_str()) {
            warnings.push(ValidationWarning {
                field: "apiProvider".to_string(),
                message: format!(
                    "Unknown apiProvider '{}'. Known providers: {}.",
                    provider,
                    VALID_API_PROVIDERS.join(", ")
                ),
                severity: WarningSeverity::Error,
            });
        }
    }

    // Validate theme
    if let Some(ref theme) = settings.theme {
        let known_themes = ["dark", "light", "auto", "solarized", "monokai", "nord"];
        if !theme.is_empty() && !known_themes.contains(&theme.to_lowercase().as_str()) {
            warnings.push(ValidationWarning {
                field: "theme".to_string(),
                message: format!(
                    "Unknown theme '{}'. Known themes: {}.",
                    theme,
                    known_themes.join(", ")
                ),
                severity: WarningSeverity::Info,
            });
        }
    }

    // Validate permission mode (legacy + nested)
    let permission_mode = settings
        .permissions
        .default_mode
        .as_ref()
        .or(settings.permission_mode.as_ref());
    if let Some(mode) = permission_mode {
        let valid = [
            "default",
            "ask",
            "auto",
            "bypass",
            "plan",
            "acceptedits",
            "dontask",
        ];
        if !valid.contains(&mode.to_ascii_lowercase().as_str()) {
            warnings.push(ValidationWarning {
                field: "permissionMode".to_string(),
                message: format!(
                    "Unknown permission mode '{}'. Known modes: {}.",
                    mode,
                    [
                        "default",
                        "ask",
                        "auto",
                        "bypass",
                        "plan",
                        "acceptEdits",
                        "dontAsk"
                    ]
                    .join(", ")
                ),
                severity: WarningSeverity::Error,
            });
        }
    }

    // Validate editor mode
    if let Some(mode) = &settings.editor_mode {
        let valid = ["normal", "vim"];
        if !valid.contains(&mode.to_lowercase().as_str()) {
            warnings.push(ValidationWarning {
                field: "editorMode".to_string(),
                message: format!(
                    "Unknown editor mode '{}'. Known modes: {}.",
                    mode,
                    valid.join(", ")
                ),
                severity: WarningSeverity::Warning,
            });
        }
    }

    if let Some(mode) = &settings.view_mode {
        let valid = ["prompt", "transcript", "focus"];
        if !valid.contains(&mode.trim().to_ascii_lowercase().as_str()) {
            warnings.push(ValidationWarning {
                field: "viewMode".to_string(),
                message: format!(
                    "Unknown viewMode '{}'. Known modes: {}. The TUI will start in prompt mode.",
                    mode,
                    valid.join(", ")
                ),
                severity: WarningSeverity::Warning,
            });
        }
    }

    if let Some(threshold) = settings.compact_threshold {
        if threshold == 0 || threshold > 100 {
            warnings.push(ValidationWarning {
                field: "compactThreshold".to_string(),
                message: format!(
                    "compactThreshold must be between 1 and 100; got {threshold}. Runtime will use the default 80% threshold."
                ),
                severity: WarningSeverity::Warning,
            });
        }
    }

    // Validate language code looks like BCP-47 / a short identifier.
    if let Some(lang) = &settings.language {
        if lang.trim().is_empty() {
            warnings.push(ValidationWarning {
                field: "language".to_string(),
                message: "Language code is set but empty.".into(),
                severity: WarningSeverity::Warning,
            });
        }
    }

    // Validate output style. Built-in names always pass; unknown names
    // are accepted but flagged Info so the user knows we'll look for a
    // matching <cwd>/.allthecodes/output-styles/<name>.md file at runtime.
    if let Some(style) = &settings.output_style {
        let trimmed = style.trim();
        if trimmed.is_empty() {
            warnings.push(ValidationWarning {
                field: "outputStyle".to_string(),
                message: "outputStyle is set but empty.".into(),
                severity: WarningSeverity::Warning,
            });
        } else {
            let lower = trimmed.to_ascii_lowercase();
            if !BUILT_IN_STYLE_NAMES.iter().any(|n| *n == lower) {
                warnings.push(ValidationWarning {
                    field: "outputStyle".to_string(),
                    message: format!(
                        "Unknown built-in style '{}'. Expected one of {}, or a custom file in .allthecodes/output-styles/.",
                        trimmed,
                        BUILT_IN_STYLE_NAMES.join(", "),
                    ),
                    severity: WarningSeverity::Info,
                });
            }
        }
    }

    // Validate effort level. Numeric overrides are accepted; unknown
    // labels are flagged Warning so the user knows they'll get the
    // default thinking budget at runtime.
    if let Some(effort) = &settings.effort_level {
        let trimmed = effort.trim();
        if !trimmed.is_empty() && !effort_label_is_known(trimmed) {
            warnings.push(ValidationWarning {
                field: "effortLevel".to_string(),
                message: format!(
                    "Unknown effort '{}'. Expected low/medium/high/xhigh/auto/max or a positive integer token count.",
                    trimmed
                ),
                severity: WarningSeverity::Warning,
            });
        }
    }

    if let Some(thinking) = &settings.thinking {
        let kind = thinking
            .get("type")
            .and_then(Value::as_str)
            .or_else(|| thinking.as_str());
        if let Some(kind) = kind {
            if !matches!(
                kind.trim().to_ascii_lowercase().as_str(),
                "enabled" | "adaptive" | "disabled"
            ) {
                warnings.push(ValidationWarning {
                    field: "thinking".to_string(),
                    message: format!(
                        "Unknown thinking type '{}'. Expected enabled, adaptive, or disabled.",
                        kind.trim()
                    ),
                    severity: WarningSeverity::Warning,
                });
            }
        } else if !thinking.is_boolean() {
            warnings.push(ValidationWarning {
                field: "thinking".to_string(),
                message: "Expected thinking to be a boolean, string, or object with a type field."
                    .to_string(),
                severity: WarningSeverity::Warning,
            });
        }
    }

    if let Some(output_config) = &settings.output_config {
        if let Some(effort) = output_config.get("effort") {
            let string_or_null = effort.as_str().is_some() || effort.is_null();
            if !string_or_null {
                warnings.push(ValidationWarning {
                    field: "output_config.effort".to_string(),
                    message:
                        "Expected output_config.effort to be a string; non-string values map to max."
                            .to_string(),
                    severity: WarningSeverity::Warning,
                });
            }
        } else if !output_config.is_object() {
            warnings.push(ValidationWarning {
                field: "output_config".to_string(),
                message: "Expected output_config to be an object.".to_string(),
                severity: WarningSeverity::Warning,
            });
        }
    }

    if let Some(effort) = &settings.model_reasoning_effort {
        let trimmed = effort.trim();
        if !trimmed.is_empty() && !model_reasoning_effort_is_known(trimmed) {
            warnings.push(ValidationWarning {
                field: "model_reasoning_effort".to_string(),
                message: format!(
                    "Unknown Codex reasoning effort '{}'. Expected none/minimal/low/medium/high/xhigh.",
                    trimmed
                ),
                severity: WarningSeverity::Warning,
            });
        }
    }

    // Validate sandbox mode
    if let Some(mode) = &settings.sandbox.mode {
        let valid = ["read-only", "workspace", "full"];
        if !valid.contains(&mode.to_lowercase().as_str()) {
            warnings.push(ValidationWarning {
                field: "sandbox.mode".to_string(),
                message: format!(
                    "Unknown sandbox mode '{}'. Known modes: {}.",
                    mode,
                    valid.join(", ")
                ),
                severity: WarningSeverity::Error,
            });
        }
    }

    warnings
}

// ---------------------------------------------------------------------------
// Extended validation — ConfigDiagnostic
// ---------------------------------------------------------------------------

/// A unified diagnostic entry combining validation warnings, permission
/// validation, and tips into a single type for use by `/doctor`.
#[derive(Debug, Clone)]
pub struct ConfigDiagnostic {
    /// The setting field or diagnostic category.
    pub field: String,
    /// Human-readable description of the issue.
    pub message: String,
    /// How serious the issue is.
    pub severity: WarningSeverity,
    /// Optional machine-readable tip code (from `ValidationTip`).
    pub code: Option<String>,
    /// Optional source context (e.g. "Managed", "User").
    pub source_info: Option<String>,
    /// Optional suggested fix.
    pub fix: Option<String>,
}

impl From<ValidationWarning> for ConfigDiagnostic {
    fn from(w: ValidationWarning) -> Self {
        Self {
            field: w.field,
            message: w.message,
            severity: w.severity,
            code: None,
            source_info: None,
            fix: None,
        }
    }
}

impl From<PermissionValidationWarning> for ConfigDiagnostic {
    fn from(w: PermissionValidationWarning) -> Self {
        Self {
            field: w.field,
            message: w.message,
            severity: w.severity,
            code: None,
            source_info: w.source_info,
            fix: None,
        }
    }
}

impl From<ValidationTip> for ConfigDiagnostic {
    fn from(t: ValidationTip) -> Self {
        Self {
            field: String::new(),
            message: t.message,
            severity: t.severity,
            code: Some(t.code),
            source_info: None,
            fix: t.fix,
        }
    }
}

/// Extended settings validation that runs the standard validator plus
/// permission validation and MDM shadowing checks.
///
/// This is the integration point called by `/doctor` and initialization
/// logic to produce a comprehensive diagnostic view.
pub fn validate_settings_extended(
    settings: &crate::runtime_settings::SettingsJson,
    loaded: &LoadedSettings,
) -> Vec<ConfigDiagnostic> {
    let mut diagnostics: Vec<ConfigDiagnostic> = validate_settings(settings)
        .into_iter()
        .map(ConfigDiagnostic::from)
        .collect();

    // Permission validation.
    let perm_warnings = permission_validation::validate_permission_settings(
        &loaded.effective.permissions,
        &loaded.sources,
    );
    for w in perm_warnings {
        let has_dup = diagnostics.iter().any(|d| d.message == w.message);
        if !has_dup {
            diagnostics.push(ConfigDiagnostic::from(w));
        }
    }

    // Permission validation tips.
    let tips = validation_tips::collect_validation_tips(&loaded.effective, &[]);
    for t in tips {
        diagnostics.push(ConfigDiagnostic::from(t));
    }

    diagnostics
}

/// Collect all config diagnostics into a unified list for `/doctor`.
///
/// Combines:
/// - Standard validation warnings.
/// - Permission validation warnings (including shadowed rules).
/// - Configuration tips.
pub fn collect_config_diagnostics(loaded: &LoadedSettings) -> Vec<ConfigDiagnostic> {
    let mut diagnostics: Vec<ConfigDiagnostic> = Vec::new();

    // Map effective settings to a SettingsJson-like view for the standard
    // validator. We can't construct a full SettingsJson here without the
    // runtime state, so we focus on diagnostics we can derive from LoadedSettings.

    // Permission validation warnings.
    let perm_warnings = permission_validation::validate_permission_settings(
        &loaded.effective.permissions,
        &loaded.sources,
    );
    for w in perm_warnings {
        diagnostics.push(ConfigDiagnostic::from(w));
    }

    add_runtime_effect_diagnostics_for_loaded(loaded, &mut diagnostics);

    // Shadowed rules.
    let managed_perms = loaded.managed.as_ref().and_then(|r| r.permissions.clone());
    let shadowed = permission_validation::find_shadowed_rules(
        &loaded.effective.permissions,
        managed_perms.as_ref(),
    );
    for rule in &shadowed {
        diagnostics.push(ConfigDiagnostic {
            field: "permissions".to_string(),
            message: format!(
                "Permission rule '{}' is shadowed by {} (source: {:?})",
                rule.rule, rule.shadowed_by, rule.source
            ),
            severity: WarningSeverity::Warning,
            code: Some("shadowed-rule".to_string()),
            source_info: Some(format!("{:?}", rule.source)),
            fix: Some(format!(
                "Review the managed policy that shadows '{}'.",
                rule.rule
            )),
        });
    }

    // Configuration tips.
    let tips = validation_tips::collect_configuration_tips(&loaded.effective);
    for t in tips {
        diagnostics.push(ConfigDiagnostic::from(t));
    }

    diagnostics
}

fn add_runtime_effect_diagnostics_for_loaded(
    loaded: &LoadedSettings,
    diagnostics: &mut Vec<ConfigDiagnostic>,
) {
    let effective = &loaded.effective;
    let mut push = |field: &str, message: String, fix: Option<String>| {
        diagnostics.push(ConfigDiagnostic {
            field: field.to_string(),
            message,
            severity: WarningSeverity::Info,
            code: Some("no-runtime-effect".to_string()),
            source_info: loaded
                .sources
                .get(field)
                .map(|source| source.as_str().to_string()),
            fix,
        });
    };

    for (field, is_set) in [
        ("appIcon", effective.app_icon.is_some()),
        ("autoStart", effective.auto_start.is_some()),
        ("startMinimized", effective.start_minimized.is_some()),
        ("minimizeToTray", effective.minimize_to_tray.is_some()),
        ("closeToTray", effective.close_to_tray.is_some()),
        (
            "quickChatHideOnBlur",
            effective.quick_chat_hide_on_blur.is_some(),
        ),
        (
            "quickChatInjectScreen",
            effective.quick_chat_inject_screen.is_some(),
        ),
        ("quickChatAmbient", effective.quick_chat_ambient.is_some()),
    ] {
        if is_set {
            push(
                field,
                format!(
                    "{field} is accepted for settings compatibility, but the Rust TUI does not support the legacy Electron behavior and this setting has no runtime effect."
                ),
                None,
            );
        }
    }

    if effective.auto_approve_tools.is_some() {
        push(
            "autoApproveTools",
            "autoApproveTools is accepted for compatibility, but it is not wired to the Rust permission system and has no runtime effect.".to_string(),
            Some("Use permissionMode or permissions.defaultMode to change tool approval behavior.".to_string()),
        );
    }
    if effective.analytics_enabled.is_some() {
        push(
            "analyticsEnabled",
            "analyticsEnabled is accepted for compatibility, but analytics collection is not implemented and this setting has no runtime effect.".to_string(),
            None,
        );
    }
    if effective.teammate_mode.is_some() {
        push(
            "teammateMode",
            "teammateMode is accepted for compatibility, but the Rust TUI does not currently implement teammate display modes and this setting has no runtime effect.".to_string(),
            None,
        );
    }

    for (field, is_set) in [
        (
            "memoryAutoRetrieve",
            effective.memory_auto_retrieve.is_some(),
        ),
        (
            "memoryQueryRewriting",
            effective.memory_query_rewriting.is_some(),
        ),
        (
            "memoryMaxRetrieved",
            effective.memory_max_retrieved.is_some(),
        ),
        (
            "memorySimilarityThreshold",
            effective.memory_similarity_threshold.is_some(),
        ),
        (
            "memoryAutoSummarize",
            effective.memory_auto_summarize.is_some(),
        ),
        ("memoryNightly", effective.memory_nightly.is_some()),
        ("memorySleepTime", effective.memory_sleep_time.is_some()),
        ("memoryTempTtl", effective.memory_temp_ttl.is_some()),
        (
            "memoryArchiveRetention",
            effective.memory_archive_retention.is_some(),
        ),
        ("memoryToolModel", effective.memory_tool_model.is_some()),
        (
            "memoryEmbeddingModel",
            effective.memory_embedding_model.is_some(),
        ),
    ] {
        if is_set {
            push(
                field,
                format!(
                    "{field} is reserved for future memory runtime tuning and currently has no runtime effect."
                ),
                None,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_anthropic_models() {
        assert!(validate_model_name("claude-opus-4-6-20250414").is_ok());
        assert!(validate_model_name("claude-sonnet-4-20250514").is_ok());
        assert!(validate_model_name("claude-haiku-4-5").is_ok());
        assert!(validate_model_name("claude-3-5-sonnet-20241022").is_ok());
        assert!(validate_model_name("claude-3-opus-20240229").is_ok());
    }

    #[test]
    fn test_valid_third_party_models() {
        assert!(validate_model_name("gpt-4o").is_ok());
        assert!(validate_model_name("o1-mini").is_ok());
        assert!(validate_model_name("gemini-1.5-pro").is_ok());
        assert!(validate_model_name("deepseek-chat").is_ok());
        assert!(validate_model_name("mistral-large-latest").is_ok());
    }

    #[test]
    fn test_valid_path_model() {
        assert!(validate_model_name("accounts/my-project/models/claude-v1").is_ok());
        assert!(validate_model_name("models/gemini-1.5-flash").is_ok());
    }

    #[test]
    fn test_invalid_models() {
        assert!(validate_model_name("").is_err());
        assert!(validate_model_name("   ").is_err());
        assert!(validate_model_name("my model name").is_err());
        assert!(validate_model_name("totally-random-string").is_err());
    }

    #[test]
    fn test_model_name_too_long() {
        let long_name = "claude-".to_string() + &"x".repeat(300);
        assert!(validate_model_name(&long_name).is_err());
    }

    #[test]
    fn test_validate_settings_empty() {
        let settings = SettingsJson::default();
        let warnings = validate_settings(&settings);
        assert!(
            warnings.is_empty(),
            "Default settings should have no warnings"
        );
    }

    #[test]
    fn test_validate_settings_bad_model() {
        let settings = SettingsJson {
            model: Some("totally invalid model".to_string()),
            ..Default::default()
        };
        let warnings = validate_settings(&settings);
        assert!(!warnings.is_empty());
        assert_eq!(warnings[0].field, "model");
        assert_eq!(warnings[0].severity, WarningSeverity::Error);
    }

    #[test]
    fn test_validate_settings_empty_model() {
        let settings = SettingsJson {
            model: Some("".to_string()),
            ..Default::default()
        };
        let warnings = validate_settings(&settings);
        assert!(!warnings.is_empty());
        assert_eq!(warnings[0].severity, WarningSeverity::Warning);
    }

    #[test]
    fn test_validate_settings_unknown_theme() {
        let settings = SettingsJson {
            theme: Some("cyberpunk".to_string()),
            ..Default::default()
        };
        let warnings = validate_settings(&settings);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "theme");
        assert_eq!(warnings[0].severity, WarningSeverity::Info);
    }

    #[test]
    fn test_validate_settings_good() {
        let settings = SettingsJson {
            model: Some("claude-sonnet-4-20250514".to_string()),
            backend: Some("codex".to_string()),
            theme: Some("dark".to_string()),
            verbose: Some(true),
            ..Default::default()
        };
        let warnings = validate_settings(&settings);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_validate_settings_bad_backend() {
        let settings = SettingsJson {
            backend: Some("mystery".to_string()),
            ..Default::default()
        };
        let warnings = validate_settings(&settings);
        assert!(!warnings.is_empty());
        assert_eq!(warnings[0].field, "backend");
        assert_eq!(warnings[0].severity, WarningSeverity::Error);
    }

    #[test]
    fn test_validate_settings_bad_permission_mode() {
        let settings = SettingsJson {
            permission_mode: Some("nope".into()),
            ..Default::default()
        };
        let warnings = validate_settings(&settings);
        assert!(warnings
            .iter()
            .any(|w| w.field == "permissionMode" && w.severity == WarningSeverity::Error));
    }

    #[test]
    fn test_validate_settings_bad_editor_mode() {
        let settings = SettingsJson {
            editor_mode: Some("emacs".into()),
            ..Default::default()
        };
        let warnings = validate_settings(&settings);
        assert!(warnings.iter().any(|w| w.field == "editorMode"));
    }

    #[test]
    fn test_validate_settings_accepts_new_permission_modes() {
        let settings = SettingsJson {
            permission_mode: Some("acceptEdits".into()),
            permissions: crate::settings::PermissionsSettings {
                default_mode: Some("dontAsk".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let warnings = validate_settings(&settings);
        assert!(
            !warnings.iter().any(|w| w.field == "permissionMode"),
            "expected new permission modes to validate cleanly, got {:?}",
            warnings
                .iter()
                .map(|w| (&w.field, &w.message))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_validate_settings_flags_electron_legacy_fields_as_info() {
        let settings = SettingsJson {
            app_icon: Some("custom".into()),
            auto_start: Some(true),
            start_minimized: Some(true),
            minimize_to_tray: Some(true),
            close_to_tray: Some(true),
            quick_chat_hide_on_blur: Some(true),
            quick_chat_inject_screen: Some(true),
            quick_chat_ambient: Some(true),
            ..Default::default()
        };

        let warnings = validate_settings(&settings);
        for field in [
            "appIcon",
            "autoStart",
            "startMinimized",
            "minimizeToTray",
            "closeToTray",
            "quickChatHideOnBlur",
            "quickChatInjectScreen",
            "quickChatAmbient",
        ] {
            assert!(
                warnings.iter().any(|warning| warning.field == field
                    && warning.severity == WarningSeverity::Info
                    && warning.message.contains("no runtime effect")),
                "missing legacy warning for {field}: {warnings:?}"
            );
        }
    }

    #[test]
    fn test_validate_settings_flags_invalid_view_and_compact_threshold() {
        let settings = SettingsJson {
            view_mode: Some("verbose".into()),
            compact_threshold: Some(101),
            ..Default::default()
        };

        let warnings = validate_settings(&settings);
        assert!(warnings.iter().any(|warning| warning.field == "viewMode"
            && warning.severity == WarningSeverity::Warning
            && warning.message.contains("prompt mode")));
        assert!(warnings
            .iter()
            .any(|warning| warning.field == "compactThreshold"
                && warning.message.contains("default 80")));
    }

    #[test]
    fn test_validate_settings_flags_no_runtime_effect_settings() {
        let settings = SettingsJson {
            auto_approve_tools: Some(true),
            analytics_enabled: Some(false),
            teammate_mode: Some(true),
            memory_auto_retrieve: Some(true),
            ..Default::default()
        };

        let warnings = validate_settings(&settings);
        for field in [
            "autoApproveTools",
            "analyticsEnabled",
            "teammateMode",
            "memoryAutoRetrieve",
        ] {
            assert!(
                warnings.iter().any(|warning| warning.field == field
                    && warning.severity == WarningSeverity::Info
                    && warning.message.contains("no runtime effect")),
                "missing no-effect warning for {field}: {warnings:?}"
            );
        }
        assert!(warnings
            .iter()
            .find(|warning| warning.field == "autoApproveTools")
            .is_some_and(|warning| warning.message.contains("permissionMode")));
    }

    #[test]
    fn test_collect_config_diagnostics_flags_auto_approve_tools_without_enabling_permissions() {
        let mut loaded = LoadedSettings::default();
        loaded.effective.auto_approve_tools = Some(true);

        let diagnostics = collect_config_diagnostics(&loaded);

        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.field == "autoApproveTools")
            .expect("autoApproveTools diagnostic");
        assert_eq!(diagnostic.severity, WarningSeverity::Info);
        assert_eq!(diagnostic.code.as_deref(), Some("no-runtime-effect"));
        assert!(diagnostic
            .fix
            .as_deref()
            .is_some_and(|fix| fix.contains("permissions.defaultMode")));
    }

    #[test]
    fn test_config_diagnostic_from_validation_warning() {
        let vw = ValidationWarning {
            field: "model".to_string(),
            message: "Invalid model".to_string(),
            severity: WarningSeverity::Error,
        };
        let cd: ConfigDiagnostic = vw.into();
        assert_eq!(cd.field, "model");
        assert_eq!(cd.message, "Invalid model");
        assert_eq!(cd.severity, WarningSeverity::Error);
        assert!(cd.code.is_none());
    }

    #[test]
    fn test_config_diagnostic_from_permission_warning() {
        let pw = PermissionValidationWarning {
            severity: WarningSeverity::Warning,
            field: "permissions.allow".to_string(),
            message: "Test warning".to_string(),
            source_info: Some("User".to_string()),
        };
        let cd: ConfigDiagnostic = pw.into();
        assert_eq!(cd.field, "permissions.allow");
        assert_eq!(cd.source_info.as_deref(), Some("User"));
    }

    #[test]
    fn test_config_diagnostic_from_validation_tip() {
        let tip = ValidationTip {
            code: "test-code".to_string(),
            message: "Test tip".to_string(),
            severity: WarningSeverity::Info,
            fix: Some("Run command".to_string()),
        };
        let cd: ConfigDiagnostic = tip.into();
        assert_eq!(cd.code.as_deref(), Some("test-code"));
        assert_eq!(cd.fix.as_deref(), Some("Run command"));
    }

    #[test]
    fn test_collect_config_diagnostics_empty_loaded() {
        use crate::settings::LoadedSettings;
        let loaded = LoadedSettings::default();
        let diagnostics = collect_config_diagnostics(&loaded);
        assert!(
            !diagnostics.is_empty(),
            "expected at least some diagnostics from empty settings, got 0"
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("missing-model")),
            "expected missing-model tip in diagnostics"
        );
    }
}
