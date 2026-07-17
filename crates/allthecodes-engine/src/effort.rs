//! Effort level to thinking budget tokens mapping.
//!
//! Used by `build_messages_request` to translate a user-facing effort
//! label or numeric override into the `thinking.budget_tokens` value
//! sent to the model API.
//!
//! The structured resolver ([`resolve_effort`]) is the provider-aware
//! source of truth: it picks a wire transport (Anthropic fixed budget
//! vs. Codex `reasoning.effort`), records where the value came from, and
//! validates it against the active profile's `supported_reasoning_levels`.
//! The legacy scalar helpers in this module (`effort_to_budget_tokens`,
//! `resolve_thinking_budget`, `normalize_output_effort_value`, ...) remain
//! for the Anthropic fixed-budget path and as compatibility shims while
//! call sites migrate onto [`ResolvedEffort`].

use allthecodes_config::settings::{
    normalize_api_provider, ModelCapabilitySettings, API_PROVIDER_ANTHROPIC,
    API_PROVIDER_OPENAI_CODEX,
};

/// Default budget when thinking is enabled but no explicit effort level is set.
pub const DEFAULT_THINKING_BUDGET: u32 = 10_240;

/// Highest fixed budget supported by the budget-based thinking path.
pub const MAX_THINKING_BUDGET: u32 = 32_768;

/// Normalize a user-provided effort value.
///
/// Accepts:
///   - `"low"` / `"medium"` / `"high"` / `"xhigh"` / `"auto"` / `"max"` (case-insensitive)
///   - `"med"` as a shorthand for `"medium"`
///   - A positive numeric string used directly as the budget
pub fn normalize_effort_value(effort: &str) -> Option<String> {
    let trimmed = effort.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(n) = trimmed.parse::<u32>() {
        if n == 0 {
            return None;
        }
        return Some(n.to_string());
    }

    match trimmed.to_ascii_lowercase().as_str() {
        "low" => Some("low".to_string()),
        "medium" | "med" => Some("medium".to_string()),
        "high" => Some("high".to_string()),
        "xhigh" => Some("xhigh".to_string()),
        "auto" => Some("auto".to_string()),
        "max" => Some("max".to_string()),
        _ => None,
    }
}

/// Normalize a value that can be sent as `output_config.effort`.
///
/// Claude's `output_config.effort` path is intentionally narrower than the
/// token-budget path. For compatibility with existing UI/settings values,
/// low/medium collapse to high, xhigh collapses to max, and any other non-empty
/// value is treated as max.
pub fn normalize_output_effort_value(effort: &str) -> Option<String> {
    let trimmed = effort.trim();
    if trimmed.is_empty() {
        return None;
    }

    match trimmed.to_ascii_lowercase().as_str() {
        "low" | "medium" | "med" | "high" => Some("high".to_string()),
        "xhigh" | "max" => Some("max".to_string()),
        _ => Some("max".to_string()),
    }
}

/// Normalize a JSON settings value into `output_config.effort`.
pub fn normalize_output_effort_json(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => normalize_output_effort_value(s),
        serde_json::Value::Null => None,
        _ => Some("max".to_string()),
    }
}

/// Map an effort label to a `thinking.budget_tokens` value.
///
/// `auto` resets to the model default budget used by this fork.
/// `max` selects the highest fixed budget supported by this budget-based path.
pub fn effort_to_budget_tokens(effort: &str) -> Option<u32> {
    let normalized = normalize_effort_value(effort)?;

    match normalized.as_str() {
        "low" => Some(4_096),
        "medium" => Some(10_240),
        "high" => Some(24_576),
        "xhigh" => Some(MAX_THINKING_BUDGET),
        "auto" => Some(DEFAULT_THINKING_BUDGET),
        "max" => Some(MAX_THINKING_BUDGET),
        numeric => numeric.parse::<u32>().ok(),
    }
}

/// Resolve the effective thinking budget for a request.
///
/// Returns the budget in priority order:
///   1. `effort_to_budget_tokens(effort_value)` if it parses
///   2. `max_output_tokens_fallback` if provided
///   3. [`DEFAULT_THINKING_BUDGET`]
pub fn resolve_thinking_budget(
    effort_value: Option<&str>,
    max_output_tokens_fallback: Option<u32>,
) -> u32 {
    effort_value
        .and_then(effort_to_budget_tokens)
        .or(max_output_tokens_fallback)
        .unwrap_or(DEFAULT_THINKING_BUDGET)
}

/// Wire transport the resolved effort is shaped for.
///
/// This is the central split that prevents cross-contamination between
/// providers: Anthropic uses a fixed-budget `thinking.budget_tokens`
/// path, Codex uses the `reasoning.effort` field, and every other
/// provider has no first-class effort shaping on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffortTransport {
    /// Anthropic fixed-budget thinking path (`thinking.budget_tokens`).
    Anthropic,
    /// OpenAI Codex `reasoning.effort` wire field.
    Codex,
    /// No first-class effort shaping on the wire (effort is local-only).
    Passthrough,
}

impl EffortTransport {
    /// Map an `api_provider` string (as stored on a provider profile) to the
    /// transport that shapes effort on the wire.
    pub fn from_api_provider(api_provider: Option<&str>) -> Self {
        match api_provider.and_then(normalize_api_provider) {
            Some(API_PROVIDER_ANTHROPIC) => EffortTransport::Anthropic,
            Some(API_PROVIDER_OPENAI_CODEX) => EffortTransport::Codex,
            _ => EffortTransport::Passthrough,
        }
    }
}

/// Where the resolved effort value came from, in priority order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffortSource {
    /// Per-turn `SubmitMessageOverrides.effort` (highest priority).
    PerTurnOverride,
    /// Active auth profile's `model_reasoning_effort` field.
    ActiveProfile,
    /// `output_config.effort` (Anthropic compat wire).
    OutputConfig,
    /// Top-level `effort_level` / `effortLevel` setting.
    EffortLevelField,
    /// `default_reasoning_level` from the bundled/user model capability.
    BundleDefault,
    /// Engine default ([`DEFAULT_THINKING_BUDGET`] label "auto").
    EngineDefault,
}

/// Whether the `supported_reasoning_levels` used to validate the level came
/// from the versioned bundled catalog or from user override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CapabilityProvenance {
    /// Default: no capability was used to validate.
    #[default]
    None,
    Bundled,
    UserProfile,
}

/// Fully-resolved effort for a single model turn.
///
/// Produced by [`resolve_effort`] and consumed by the request builder to
/// decide what (if anything) to place in `thinking`, `output_config.effort`,
/// and `MessagesRequest.reasoning_effort`. Carrying the resolved value on
/// `ModelCallParams` (as `resolved_effort`) lets the wire builder avoid
/// re-deriving priority at request time and prevents the per-turn override
/// from being silently down-ranked by a profile baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEffort {
    /// Canonical effort label actually applied (e.g. "low", "high", "xhigh",
    /// "max"). For numeric overrides on the Anthropic path this is the raw
    /// numeric string; for Codex it is always one of the supported wire
    /// labels.
    pub level: String,
    /// Origin of `level`, for diagnostics and `/effort` display.
    pub source: EffortSource,
    /// Wire transport this effort is shaped for.
    pub transport: EffortTransport,
    /// Local `thinking.budget_tokens` for the Anthropic fixed-budget path.
    /// `None` for Codex/Passthrough transports (no local budget shaping).
    pub local_budget_tokens: Option<u32>,
    /// Provenance of the `supported_reasoning_levels` used to validate.
    pub capability_provenance: CapabilityProvenance,
}

/// Inputs to [`resolve_effort`].
///
/// All of the candidate effort sources are passed in explicitly so the
/// resolver has a single priority chain and a single place to record
/// provenance. Callers shuttle these from `SubmitMessageOverrides`,
/// `AppState`, and the active provider profile.
#[derive(Debug, Clone, Default)]
pub struct ResolveEffortInput<'a> {
    /// Per-turn override (`SubmitMessageOverrides.effort`), highest priority.
    pub per_turn_override: Option<&'a str>,
    /// Active profile's persisted `model_reasoning_effort`.
    pub profile_model_reasoning_effort: Option<&'a str>,
    /// `output_config.effort` (Anthropic compat web wire field).
    pub output_config_effort: Option<&'a str>,
    /// Top-level `effort_level` setting.
    pub effort_level: Option<&'a str>,
    /// `app_state.effort_value` snapshot (legacy unified field; treated as a
    /// per-turn-equivalent when no explicit override is present, to preserve
    /// `/effort`-set behavior across turns).
    pub effort_value: Option<&'a str>,
    /// `api_provider` of the active profile (selects the transport).
    pub api_provider: Option<&'a str>,
    /// Active model's capability (provides `supported_reasoning_levels` and
    /// `default_reasoning_level`). `None` when no capability is configured.
    pub capability: Option<&'a ModelCapabilitySettings>,
    /// Whether `capability` came from the bundled catalog or user override.
    pub capability_provenance: CapabilityProvenance,
}

impl ResolvedEffort {
    /// Canonical "auto" label used when no effort is configured.
    pub const AUTO_LABEL: &'static str = "auto";

    /// True when this resolution should drive `output_config.effort`
    /// (Anthropic compat wire). Codex/Passthrough must NOT touch it.
    pub fn writes_output_config_effort(&self) -> bool {
        matches!(self.transport, EffortTransport::Anthropic)
    }

    /// True when this resolution should drive
    /// `MessagesRequest.reasoning_effort` (Codex wire). Anthropic/Passthrough
    /// must NOT set it from effort.
    pub fn writes_reasoning_effort(&self) -> bool {
        matches!(self.transport, EffortTransport::Codex)
    }

    /// True when this resolution should drive `thinking.budget_tokens`
    /// (Anthropic fixed-budget). Codex/Passthrough must NOT.
    pub fn writes_thinking_budget(&self) -> bool {
        matches!(self.transport, EffortTransport::Anthropic)
    }
}

/// Resolve effort from the candidate inputs following the fixed priority
/// chain, validated against the active model's `supported_reasoning_levels`.
///
/// Priority (first non-empty wins):
///   1. [`EffortSource::PerTurnOverride`] -- `per_turn_override` if present
///   2. [`EffortSource::ActiveProfile`] -- `profile_model_reasoning_effort`
///      (the persisted Codex profile baseline)
///   3. [`EffortSource::OutputConfig`] -- `output_config_effort`
///   4. [`EffortSource::EffortLevelField`] -- `effort_level`
///   5. `effort_value` (legacy unified field, treated as PerTurnOverride-ish
///      so existing `/effort`-set flows keep working across turns)
///   6. [`EffortSource::BundleDefault`] -- `capability.default_reasoning_level`
///   7. [`EffortSource::EngineDefault`] -- "auto"
///
/// If the chosen level is not in `supported_reasoning_levels` (and the
/// capability is present with levels), the resolver falls back to the
/// capability default rather than silently coercing or dropping the value.
/// An explicit user override that is unsupported by the active model yields
/// `None` from the candidate (it was rejected at the command/picker layer
/// before reaching here); the resolver only rejects-levels that are
/// *implied* by a non-validated field (e.g. profile baseline vs. a
/// different model).
pub fn resolve_effort(input: ResolveEffortInput<'_>) -> ResolvedEffort {
    let transport = EffortTransport::from_api_provider(input.api_provider);
    let supported = input
        .capability
        .map(|cap| cap.supported_reasoning_levels.as_slice())
        .unwrap_or(&[]);

    // Candidate chain: (label, source). Selection is *transport-gated* so
    // cross-provider fields cannot leak across the wire boundary:
    //   - Codex        : per-turn override, active profile baseline, legacy
    //                    `effort_value`, BundleDefault. NEVER `output_config`
    //                    or top-level `effort_level` (Anthropic-only fields).
    //   - Anthropic    : per-turn override, `output_config.effort`, top-level
    //                    `effort_level`, legacy `effort_value`, BundleDefault.
    //                    NEVER the active profile's `model_reasoning_effort`
    //                    (Codex-only baseline).
    //   - Passthrough  : per-turn override only, else BundleDefault.
    //
    // `output_config.effort` on a Codex profile, and `model_reasoning_effort`
    // on an Anthropic profile, are intentionally NOT consumed here -- per the
    // plan §3.1 contract, this is the fix that stops the legacy
    // Anthropic-collapse collapse from masquerading as a Codex level.
    let candidates: Vec<Option<(&str, EffortSource)>> = match transport {
        EffortTransport::Codex => vec![
            input
                .per_turn_override
                .map(|v| (v, EffortSource::PerTurnOverride)),
            input
                .profile_model_reasoning_effort
                .map(|v| (v, EffortSource::ActiveProfile)),
            input
                .effort_value
                .map(|v| (v, EffortSource::PerTurnOverride)),
        ],
        EffortTransport::Anthropic => vec![
            input
                .per_turn_override
                .map(|v| (v, EffortSource::PerTurnOverride)),
            input
                .output_config_effort
                .map(|v| (v, EffortSource::OutputConfig)),
            input
                .effort_level
                .map(|v| (v, EffortSource::EffortLevelField)),
            input
                .effort_value
                .map(|v| (v, EffortSource::PerTurnOverride)),
        ],
        EffortTransport::Passthrough => vec![input
            .per_turn_override
            .map(|v| (v, EffortSource::PerTurnOverride))],
    };

    let capability_provenance = if input.capability.is_some() {
        input.capability_provenance
    } else {
        CapabilityProvenance::None
    };

    for (label, source) in candidates.into_iter().flatten() {
        let normalized = normalize_effort_value(label);
        if let Some(canonical) = normalized {
            if is_supported_level(&canonical, supported) {
                return finalize(canonical, source, transport, capability_provenance);
            }
            // Unsupported level for this model: only fall through to the next
            // candidate when the value came from an *ambient* field (profile
            // baseline, output_config, effort_level, effort_value). A true
            // per-turn override that is unsupported is a caller bug -- it
            // should have been rejected at the command layer -- so we let it
            // drop to BundleDefault rather than silently coerce.
        }
    }

    // Bundle default, then engine default.
    if let Some(cap_level) = input
        .capability
        .and_then(|cap| cap.default_reasoning_level.as_deref())
        .filter(|level| is_supported_level(level, supported))
    {
        return finalize(
            cap_level.to_string(),
            EffortSource::BundleDefault,
            transport,
            capability_provenance,
        );
    }

    finalize(
        ResolvedEffort::AUTO_LABEL.to_string(),
        EffortSource::EngineDefault,
        transport,
        capability_provenance,
    )
}

fn is_supported_level(candidate: &str, supported: &[String]) -> bool {
    if supported.is_empty() {
        return true; // No capability configured: accept any canonical label.
    }
    supported.iter().any(|level| level == candidate)
}

/// Snapshot the active profile's effort-relevant inputs from `AppState` and
/// resolve them through the central resolver.
///
/// This is the single entry point all UI surfaces (effort picker, status
/// widget, model detail) should call so the displayed value matches the wire.
/// The caller decides which profile is "active"; we only read its
/// `api_provider` (for transport), its `model_reasoning_effort` (for the
/// Codex baseline candidate), and the capability for the requested model.
pub fn resolve_effort_for_state(
    api_provider: Option<&str>,
    profile_model_reasoning_effort: Option<&str>,
    output_config_effort: Option<&str>,
    effort_level: Option<&str>,
    effort_value: Option<&str>,
    capability: Option<&allthecodes_config::settings::ModelCapabilitySettings>,
    capability_provenance: CapabilityProvenance,
) -> ResolvedEffort {
    resolve_effort(ResolveEffortInput {
        api_provider,
        per_turn_override: None,
        profile_model_reasoning_effort,
        output_config_effort,
        effort_level,
        effort_value,
        capability,
        capability_provenance,
    })
}

fn finalize(
    level: String,
    source: EffortSource,
    transport: EffortTransport,
    capability_provenance: CapabilityProvenance,
) -> ResolvedEffort {
    let local_budget_tokens = match transport {
        EffortTransport::Anthropic => effort_to_budget_tokens(&level),
        EffortTransport::Codex | EffortTransport::Passthrough => None,
    };

    ResolvedEffort {
        level,
        source,
        transport,
        local_budget_tokens,
        capability_provenance,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_labels_map_to_budgets() {
        assert_eq!(effort_to_budget_tokens("low"), Some(4_096));
        assert_eq!(effort_to_budget_tokens("medium"), Some(10_240));
        assert_eq!(effort_to_budget_tokens("high"), Some(24_576));
        assert_eq!(effort_to_budget_tokens("xhigh"), Some(MAX_THINKING_BUDGET));
    }

    #[test]
    fn auto_and_max_map_to_supported_budgets() {
        assert_eq!(
            effort_to_budget_tokens("auto"),
            Some(DEFAULT_THINKING_BUDGET)
        );
        assert_eq!(effort_to_budget_tokens("max"), Some(MAX_THINKING_BUDGET));
    }

    #[test]
    fn labels_are_case_insensitive() {
        assert_eq!(effort_to_budget_tokens("LOW"), Some(4_096));
        assert_eq!(effort_to_budget_tokens("Medium"), Some(10_240));
        assert_eq!(effort_to_budget_tokens("HIGH"), Some(24_576));
        assert_eq!(effort_to_budget_tokens("XHIGH"), Some(MAX_THINKING_BUDGET));
        assert_eq!(
            effort_to_budget_tokens("AUTO"),
            Some(DEFAULT_THINKING_BUDGET)
        );
        assert_eq!(effort_to_budget_tokens("MAX"), Some(MAX_THINKING_BUDGET));
    }

    #[test]
    fn numeric_override_returned_directly() {
        assert_eq!(effort_to_budget_tokens("4096"), Some(4_096));
        assert_eq!(effort_to_budget_tokens("32000"), Some(32_000));
        assert_eq!(effort_to_budget_tokens(" 8000 "), Some(8_000));
    }

    #[test]
    fn zero_and_empty_return_none() {
        assert_eq!(effort_to_budget_tokens(""), None);
        assert_eq!(effort_to_budget_tokens("   "), None);
        assert_eq!(effort_to_budget_tokens("0"), None);
    }

    #[test]
    fn unknown_labels_return_none() {
        assert_eq!(effort_to_budget_tokens("ultra"), None);
        assert_eq!(effort_to_budget_tokens("not-a-number-or-label"), None);
    }

    #[test]
    fn normalize_effort_value_canonicalizes_labels() {
        assert_eq!(normalize_effort_value("med"), Some("medium".to_string()));
        assert_eq!(normalize_effort_value("XHIGH"), Some("xhigh".to_string()));
        assert_eq!(normalize_effort_value(" AUTO "), Some("auto".to_string()));
        assert_eq!(normalize_effort_value(" 12000 "), Some("12000".to_string()));
    }

    #[test]
    fn output_effort_maps_to_claude_supported_values() {
        assert_eq!(
            normalize_output_effort_value("med"),
            Some("high".to_string())
        );
        assert_eq!(
            normalize_output_effort_value("low"),
            Some("high".to_string())
        );
        assert_eq!(
            normalize_output_effort_value("medium"),
            Some("high".to_string())
        );
        assert_eq!(
            normalize_output_effort_value("high"),
            Some("high".to_string())
        );
        assert_eq!(
            normalize_output_effort_value("xhigh"),
            Some("max".to_string())
        );
        assert_eq!(
            normalize_output_effort_value("max"),
            Some("max".to_string())
        );
        assert_eq!(
            normalize_output_effort_value("12000"),
            Some("max".to_string())
        );
        assert_eq!(
            normalize_output_effort_value("auto"),
            Some("max".to_string())
        );
        assert_eq!(
            normalize_output_effort_value("ultra"),
            Some("max".to_string())
        );
        assert_eq!(normalize_output_effort_value("   "), None);
    }

    #[test]
    fn output_effort_json_maps_non_string_values_to_max() {
        assert_eq!(
            normalize_output_effort_json(&serde_json::json!("low")),
            Some("high".to_string())
        );
        assert_eq!(
            normalize_output_effort_json(&serde_json::json!(12000)),
            Some("max".to_string())
        );
        assert_eq!(normalize_output_effort_json(&serde_json::Value::Null), None);
    }

    #[test]
    fn resolve_prefers_effort_label() {
        assert_eq!(resolve_thinking_budget(Some("high"), Some(99_999)), 24_576);
    }

    #[test]
    fn resolve_supports_auto_and_max() {
        assert_eq!(
            resolve_thinking_budget(Some("auto"), Some(99_999)),
            DEFAULT_THINKING_BUDGET
        );
        assert_eq!(
            resolve_thinking_budget(Some("max"), Some(99_999)),
            MAX_THINKING_BUDGET
        );
    }

    #[test]
    fn resolve_falls_back_to_max_tokens() {
        assert_eq!(resolve_thinking_budget(None, Some(8_000)), 8_000);
        assert_eq!(resolve_thinking_budget(Some("ultra"), Some(8_000)), 8_000);
    }

    #[test]
    fn resolve_falls_back_to_default() {
        assert_eq!(resolve_thinking_budget(None, None), DEFAULT_THINKING_BUDGET);
        assert_eq!(
            resolve_thinking_budget(Some(""), None),
            DEFAULT_THINKING_BUDGET
        );
    }

    // ---- resolve_effort: provider-aware resolver ----

    fn codex_cap(levels: &[&str], default: Option<&str>) -> ModelCapabilitySettings {
        ModelCapabilitySettings {
            default_reasoning_level: default.map(|s| s.to_string()),
            supported_reasoning_levels: levels.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn codex_gpt56_cap() -> ModelCapabilitySettings {
        codex_cap(&["low", "medium", "high", "xhigh", "max"], Some("medium"))
    }

    fn anthropic_cap() -> ModelCapabilitySettings {
        codex_cap(&["low", "medium", "high", "xhigh"], Some("medium"))
    }

    #[test]
    fn per_turn_override_beats_profile_baseline_for_codex() {
        // Regression for the priority-broken resolver: explicit per-turn
        // override must win over the profile model_reasoning_effort baseline
        // so SubmitMessageOverrides.effort is not silently down-ranked.
        let cap = codex_gpt56_cap();
        let resolved = resolve_effort(ResolveEffortInput {
            per_turn_override: Some("high"),
            profile_model_reasoning_effort: Some("low"),
            api_provider: Some("openai-codex"),
            capability: Some(&cap),
            capability_provenance: CapabilityProvenance::Bundled,
            ..Default::default()
        });
        assert_eq!(resolved.level, "high");
        assert_eq!(resolved.source, EffortSource::PerTurnOverride);
        assert_eq!(resolved.transport, EffortTransport::Codex);
        assert!(resolved.writes_reasoning_effort());
        assert!(!resolved.writes_output_config_effort());
        assert!(!resolved.writes_thinking_budget());
        // Codex never carries a local thinking budget.
        assert_eq!(resolved.local_budget_tokens, None);
    }

    #[test]
    fn codex_resolver_emits_xhigh_directly() {
        // Regression for codex_effort_from_effort_level dropping "xhigh".
        let cap = codex_gpt56_cap();
        let resolved = resolve_effort(ResolveEffortInput {
            per_turn_override: Some("xhigh"),
            api_provider: Some("openai-codex"),
            capability: Some(&cap),
            capability_provenance: CapabilityProvenance::Bundled,
            ..Default::default()
        });
        assert_eq!(resolved.level, "xhigh");
        assert_eq!(resolved.transport, EffortTransport::Codex);
    }

    #[test]
    fn codex_max_stays_max_for_5_6_family() {
        let cap = codex_gpt56_cap();
        let resolved = resolve_effort(ResolveEffortInput {
            per_turn_override: Some("max"),
            api_provider: Some("openai-codex"),
            capability: Some(&cap),
            capability_provenance: CapabilityProvenance::Bundled,
            ..Default::default()
        });
        assert_eq!(resolved.level, "max");
    }

    #[test]
    fn codex_max_rejected_for_5_5_falls_back_to_default() {
        // gpt-5.5 does not support "max" -- an ambient field claiming "max"
        // must fall back to the capability default (medium) rather than coerce.
        let cap = codex_cap(&["low", "medium", "high", "xhigh"], Some("medium"));
        let resolved = resolve_effort(ResolveEffortInput {
            profile_model_reasoning_effort: Some("max"),
            api_provider: Some("openai-codex"),
            capability: Some(&cap),
            capability_provenance: CapabilityProvenance::Bundled,
            ..Default::default()
        });
        assert_eq!(resolved.level, "medium");
        assert_eq!(resolved.source, EffortSource::BundleDefault);
    }

    #[test]
    fn anthropic_transport_carries_local_budget() {
        let cap = anthropic_cap();
        let resolved = resolve_effort(ResolveEffortInput {
            per_turn_override: Some("high"),
            api_provider: Some("anthropic"),
            capability: Some(&cap),
            capability_provenance: CapabilityProvenance::Bundled,
            ..Default::default()
        });
        assert_eq!(resolved.level, "high");
        assert_eq!(resolved.transport, EffortTransport::Anthropic);
        assert_eq!(resolved.local_budget_tokens, Some(24_576));
        assert!(resolved.writes_thinking_budget());
        assert!(resolved.writes_output_config_effort());
        assert!(!resolved.writes_reasoning_effort());
    }

    #[test]
    fn unknown_provider_is_passthrough_no_wire_writes() {
        let resolved = resolve_effort(ResolveEffortInput {
            per_turn_override: Some("high"),
            api_provider: Some("openrouter"),
            capability_provenance: CapabilityProvenance::None,
            ..Default::default()
        });
        assert_eq!(resolved.transport, EffortTransport::Passthrough);
        assert!(!resolved.writes_thinking_budget());
        assert!(!resolved.writes_output_config_effort());
        assert!(!resolved.writes_reasoning_effort());
        assert_eq!(resolved.local_budget_tokens, None);
    }

    #[test]
    fn empty_input_falls_to_engine_default_auto() {
        let resolved = resolve_effort(ResolveEffortInput {
            api_provider: Some("openai-codex"),
            capability_provenance: CapabilityProvenance::None,
            ..Default::default()
        });
        assert_eq!(resolved.level, ResolvedEffort::AUTO_LABEL);
        assert_eq!(resolved.source, EffortSource::EngineDefault);
    }

    #[test]
    fn effort_value_unified_field_is_respected_as_override_equivalent() {
        // Regression for the /effort-set flow: app_state.effort_value (the
        // legacy unified field) must drive the resolution when no per-turn
        // override is present.
        let cap = codex_gpt56_cap();
        let resolved = resolve_effort(ResolveEffortInput {
            effort_value: Some("high"),
            api_provider: Some("openai-codex"),
            capability: Some(&cap),
            capability_provenance: CapabilityProvenance::Bundled,
            ..Default::default()
        });
        assert_eq!(resolved.level, "high");
    }

    #[test]
    fn profile_baseline_beats_output_config_for_codex() {
        // For Codex, the persisted model_reasoning_effort is the canonical
        // baseline; output_config.effort is Anthropic-compat and must NOT
        // leak into Codex reasoning.effort.
        let cap = codex_gpt56_cap();
        let resolved = resolve_effort(ResolveEffortInput {
            profile_model_reasoning_effort: Some("low"),
            output_config_effort: Some("max"), // would be wrong for Codex
            api_provider: Some("openai-codex"),
            capability: Some(&cap),
            capability_provenance: CapabilityProvenance::Bundled,
            ..Default::default()
        });
        assert_eq!(resolved.level, "low");
        assert_eq!(resolved.source, EffortSource::ActiveProfile);
    }
}
