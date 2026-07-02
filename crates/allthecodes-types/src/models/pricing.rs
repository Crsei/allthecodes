//! Model token pricing built-in table plus environment variable overrides.

use serde::{Deserialize, Serialize};

/// Price per 1 million tokens in USD.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ModelPricing {
    pub input_per_1m: f64,
    pub output_per_1m: f64,
}

impl ModelPricing {
    pub const ZERO: ModelPricing = ModelPricing {
        input_per_1m: 0.0,
        output_per_1m: 0.0,
    };

    /// Calculate cost in USD from token counts.
    pub fn cost_from_counts(
        &self,
        input_tokens: u64,
        output_tokens: u64,
        cache_read_input_tokens: u64,
        cache_creation_input_tokens: u64,
    ) -> f64 {
        cost_from_counts(
            *self,
            input_tokens,
            output_tokens,
            cache_read_input_tokens,
            cache_creation_input_tokens,
        )
    }
}

/// Source of pricing information for a cost calculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingSource {
    /// Matched from the built-in pricing table.
    Builtin,
    /// Overridden by MODEL_INPUT_PRICE / MODEL_OUTPUT_PRICE env vars.
    EnvOverride,
    /// Provider reported cost (future use).
    ProviderReported,
    /// Model not found in pricing table, zero pricing used.
    Unknown,
    /// Backfilled from session assistant messages (no runtime event).
    Backfilled,
}

/// The result of a pricing lookup, including the matched key and source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingMatch {
    /// Source of the pricing.
    pub source: PricingSource,
    /// The matched model key (empty when unknown).
    pub matched_key: String,
    /// The pricing row that was matched (zero when unknown).
    pub pricing: ModelPricing,
}

/// Full cost breakdown including pricing metadata and per-component costs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostBreakdown {
    /// The pricing match that was used.
    pub pricing: PricingMatch,
    /// Cost of input tokens.
    pub input_cost: f64,
    /// Cost of output tokens.
    pub output_cost: f64,
    /// Cost of cache read tokens (at 0.1x input price).
    pub cache_read_cost: f64,
    /// Cost of cache creation tokens (at 1.25x input price).
    pub cache_creation_cost: f64,
    /// Total cost (sum of all components).
    pub total_cost: f64,
}

/// (model_id_prefix, input_per_1m, output_per_1m)
const BUILTIN_PRICING: &[(&str, f64, f64)] = &[
    ("claude-opus-4-6", 5.0, 25.0),
    ("claude-opus-4-0", 15.0, 75.0),
    ("claude-opus-4", 5.0, 25.0),
    ("claude-sonnet-4-6", 3.0, 15.0),
    ("claude-sonnet-4-0", 3.0, 15.0),
    ("claude-sonnet-4", 3.0, 15.0),
    ("claude-haiku-4-5", 1.0, 5.0),
    ("claude-haiku-3-5", 1.0, 5.0),
    ("gpt-4o-mini", 0.15, 0.60),
    ("gpt-4o", 2.50, 10.0),
    ("gpt-4.1-nano", 0.10, 0.40),
    ("gpt-4.1-mini", 0.20, 0.80),
    ("gpt-4.1", 2.0, 8.0),
    ("gpt-5-nano", 0.10, 0.40),
    ("o4-mini", 0.55, 2.20),
    ("o3-mini", 1.10, 4.40),
    ("o3", 2.0, 8.0),
    ("gemini-2.5-pro", 1.25, 10.0),
    ("gemini-2.5-flash", 0.30, 2.50),
    ("gemini-2.0-flash", 0.10, 0.40),
    ("deepseek-chat", 0.28, 0.42),
    ("deepseek-reasoner", 0.55, 2.19),
];

/// Look up pricing and return a structured `PricingMatch` (not just the raw row).
pub fn get_pricing_match(model: &str) -> PricingMatch {
    if let Some(pricing) = pricing_from_env() {
        return PricingMatch {
            source: PricingSource::EnvOverride,
            matched_key: "__env_override__".to_string(),
            pricing,
        };
    }
    if let Some((key, pricing)) = pricing_from_table_with_key(model) {
        return PricingMatch {
            source: PricingSource::Builtin,
            matched_key: key.to_string(),
            pricing,
        };
    }
    PricingMatch {
        source: PricingSource::Unknown,
        matched_key: String::new(),
        pricing: ModelPricing::ZERO,
    }
}

/// Look up pricing for a model. Returns the raw pricing row.
///
/// Prefer `get_pricing_match` when the caller needs to know the source or
/// matched key. This function exists for backward compatibility.
pub fn get_pricing(model: &str) -> ModelPricing {
    get_pricing_match(model).pricing
}

/// Calculate full `CostBreakdown` from pricing match and usage.
pub fn calculate_cost_breakdown(
    pricing: &PricingMatch,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_input_tokens: u64,
    cache_creation_input_tokens: u64,
) -> CostBreakdown {
    let p = pricing.pricing;
    let input_cost = input_tokens as f64 * p.input_per_1m / 1_000_000.0;
    let output_cost = output_tokens as f64 * p.output_per_1m / 1_000_000.0;
    let cache_read_cost = cache_read_input_tokens as f64 * p.input_per_1m * 0.1 / 1_000_000.0;
    let cache_creation_cost =
        cache_creation_input_tokens as f64 * p.input_per_1m * 1.25 / 1_000_000.0;
    let total_cost = input_cost + output_cost + cache_read_cost + cache_creation_cost;

    CostBreakdown {
        pricing: pricing.clone(),
        input_cost,
        output_cost,
        cache_read_cost,
        cache_creation_cost,
        total_cost,
    }
}

/// Calculate total cost in USD from a pricing row and token counts.
pub fn cost_from_counts(
    pricing: ModelPricing,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_input_tokens: u64,
    cache_creation_input_tokens: u64,
) -> f64 {
    let input_cost = input_tokens as f64 * pricing.input_per_1m / 1_000_000.0;
    let output_cost = output_tokens as f64 * pricing.output_per_1m / 1_000_000.0;
    let cache_read_cost = cache_read_input_tokens as f64 * pricing.input_per_1m * 0.1 / 1_000_000.0;
    let cache_write_cost =
        cache_creation_input_tokens as f64 * pricing.input_per_1m * 1.25 / 1_000_000.0;
    input_cost + output_cost + cache_read_cost + cache_write_cost
}

fn pricing_from_env() -> Option<ModelPricing> {
    let input = std::env::var("MODEL_INPUT_PRICE")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())?;
    let output = std::env::var("MODEL_OUTPUT_PRICE")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())?;
    Some(ModelPricing {
        input_per_1m: input,
        output_per_1m: output,
    })
}

/// Thin wrapper around `pricing_from_table_with_key` that discards the key.
#[cfg(test)]
pub(crate) fn pricing_from_table(model: &str) -> Option<ModelPricing> {
    pricing_from_table_with_key(model).map(|(_, p)| p)
}

fn pricing_from_table_with_key(model: &str) -> Option<(&'static str, ModelPricing)> {
    let lower = model.to_lowercase();

    for &(prefix, input, output) in BUILTIN_PRICING {
        if lower == prefix {
            return Some((
                prefix,
                ModelPricing {
                    input_per_1m: input,
                    output_per_1m: output,
                },
            ));
        }
    }

    for &(prefix, input, output) in BUILTIN_PRICING {
        if lower.starts_with(prefix) {
            return Some((
                prefix,
                ModelPricing {
                    input_per_1m: input,
                    output_per_1m: output,
                },
            ));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static PRICING_ENV_LOCK: Mutex<()> = Mutex::new(());

    struct PricingEnvSnapshot {
        input: Option<String>,
        output: Option<String>,
    }

    impl PricingEnvSnapshot {
        fn clear() -> Self {
            let snapshot = Self::capture();
            std::env::remove_var("MODEL_INPUT_PRICE");
            std::env::remove_var("MODEL_OUTPUT_PRICE");
            snapshot
        }

        fn override_with(input: &str, output: &str) -> Self {
            let snapshot = Self::capture();
            std::env::set_var("MODEL_INPUT_PRICE", input);
            std::env::set_var("MODEL_OUTPUT_PRICE", output);
            snapshot
        }

        fn capture() -> Self {
            Self {
                input: std::env::var("MODEL_INPUT_PRICE").ok(),
                output: std::env::var("MODEL_OUTPUT_PRICE").ok(),
            }
        }
    }

    impl Drop for PricingEnvSnapshot {
        fn drop(&mut self) {
            match &self.input {
                Some(v) => std::env::set_var("MODEL_INPUT_PRICE", v),
                None => std::env::remove_var("MODEL_INPUT_PRICE"),
            }
            match &self.output {
                Some(v) => std::env::set_var("MODEL_OUTPUT_PRICE", v),
                None => std::env::remove_var("MODEL_OUTPUT_PRICE"),
            }
        }
    }

    fn lock_pricing_env() -> MutexGuard<'static, ()> {
        PRICING_ENV_LOCK
            .lock()
            .expect("pricing env test lock poisoned")
    }

    #[test]
    fn exact_match_claude_sonnet() {
        let p = pricing_from_table("claude-sonnet-4-20250514").unwrap();
        assert_eq!(p.input_per_1m, 3.0);
        assert_eq!(p.output_per_1m, 15.0);
    }

    #[test]
    fn exact_match_gpt4o() {
        let p = pricing_from_table("gpt-4o").unwrap();
        assert_eq!(p.input_per_1m, 2.5);
        assert_eq!(p.output_per_1m, 10.0);
    }

    #[test]
    fn prefix_match_gpt4o_dated() {
        let p = pricing_from_table("gpt-4o-2024-11-20").unwrap();
        assert_eq!(p.input_per_1m, 2.5);
    }

    #[test]
    fn gpt5_nano_known() {
        let p = pricing_from_table("gpt-5-nano").unwrap();
        assert_eq!(p.input_per_1m, 0.10);
        assert_eq!(p.output_per_1m, 0.40);
    }

    #[test]
    fn unknown_model_returns_none() {
        assert!(pricing_from_table("totally-unknown-model").is_none());
    }

    #[test]
    fn case_insensitive() {
        let p = pricing_from_table("Claude-Sonnet-4-20250514").unwrap();
        assert_eq!(p.input_per_1m, 3.0);
    }

    #[test]
    fn calculate_cost_basic() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let cost = cost_from_counts(get_pricing("gpt-4o"), 1000, 500, 0, 0);
        let expected = 1000.0 * 2.5 / 1_000_000.0 + 500.0 * 10.0 / 1_000_000.0;
        assert!((cost - expected).abs() < 1e-10);
    }

    #[test]
    fn calculate_cost_unknown_model_is_zero() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let cost = cost_from_counts(get_pricing("unknown-model"), 1000, 500, 0, 0);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn env_override_takes_precedence() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::override_with("99.0", "199.0");
        let p = get_pricing("gpt-4o");
        assert_eq!(p.input_per_1m, 99.0);
        assert_eq!(p.output_per_1m, 199.0);
    }

    #[test]
    fn cache_read_cost_calculation() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let pricing = get_pricing("claude-sonnet-4");
        let cost = cost_from_counts(pricing, 0, 0, 1000000, 0);
        let expected = 1_000_000.0 * 3.0 * 0.1 / 1_000_000.0;
        assert!((cost - expected).abs() < 1e-10);
        let pricing2 = get_pricing("gpt-4o");
        let cost2 = cost_from_counts(pricing2, 0, 0, 2000000, 0);
        let expected2 = 2_000_000.0 * 2.5 * 0.1 / 1_000_000.0;
        assert!((cost2 - expected2).abs() < 1e-10);
    }

    #[test]
    fn cache_creation_cost_calculation() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let pricing = get_pricing("claude-sonnet-4");
        let cost = cost_from_counts(pricing, 0, 0, 0, 1000000);
        let expected = 1_000_000.0 * 3.0 * 1.25 / 1_000_000.0;
        assert!((cost - expected).abs() < 1e-10);
        let pricing2 = get_pricing("gpt-4o");
        let cost2 = cost_from_counts(pricing2, 0, 0, 0, 2000000);
        let expected2 = 2_000_000.0 * 2.5 * 1.25 / 1_000_000.0;
        assert!((cost2 - expected2).abs() < 1e-10);
    }

    #[test]
    fn reasoning_tokens_do_not_affect_cost() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let pricing = get_pricing("claude-sonnet-4");
        let cost_with_reasoning = cost_from_counts(pricing, 500, 200, 100, 50);
        let cost_expected = 500.0 * 3.0 / 1_000_000.0
            + 200.0 * 15.0 / 1_000_000.0
            + 100.0 * 3.0 * 0.1 / 1_000_000.0
            + 50.0 * 3.0 * 1.25 / 1_000_000.0;
        assert!((cost_with_reasoning - cost_expected).abs() < 1e-10);
    }

    #[test]
    fn zero_input_pricing() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let pricing = get_pricing("completely-fake-model-xyz");
        assert_eq!(pricing.input_per_1m, 0.0);
        assert_eq!(pricing.output_per_1m, 0.0);
        let cost = cost_from_counts(pricing, 50000, 10000, 20000, 30000);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn cost_from_counts_with_all_fields() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let pricing = get_pricing("gpt-4o");
        let cost = cost_from_counts(pricing, 1000, 500, 200, 100);
        let expected = 1000.0 * 2.5 / 1_000_000.0
            + 500.0 * 10.0 / 1_000_000.0
            + 200.0 * 2.5 * 0.1 / 1_000_000.0
            + 100.0 * 2.5 * 1.25 / 1_000_000.0;
        assert!((cost - expected).abs() < 1e-10);
        let cost_basic = cost_from_counts(pricing, 1000, 500, 0, 0);
        assert!(cost > cost_basic);
    }

    #[test]
    fn env_override_marks_pricing_unknown() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::override_with("7.0", "14.0");
        let p = get_pricing("unknown-model-123");
        assert_eq!(p.input_per_1m, 7.0);
        assert_eq!(p.output_per_1m, 14.0);
        let cost = cost_from_counts(p, 1000000, 500000, 0, 0);
        let expected = 1_000_000.0 * 7.0 / 1_000_000.0 + 500_000.0 * 14.0 / 1_000_000.0;
        assert!((cost - expected).abs() < 1e-10);
    }

    #[test]
    fn get_pricing_match_returns_builtin() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let m = get_pricing_match("gpt-4o");
        assert_eq!(m.source, PricingSource::Builtin);
        assert_eq!(m.matched_key, "gpt-4o");
        assert_eq!(m.pricing.input_per_1m, 2.5);
    }

    #[test]
    fn get_pricing_match_returns_builtin_with_dated_model() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let m = get_pricing_match("claude-sonnet-4-20250514");
        assert_eq!(m.source, PricingSource::Builtin);
        assert_eq!(m.matched_key, "claude-sonnet-4");
        assert_eq!(m.pricing.input_per_1m, 3.0);
    }

    #[test]
    fn get_pricing_match_returns_env_override() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::override_with("42.0", "84.0");
        let m = get_pricing_match("gpt-4o");
        assert_eq!(m.source, PricingSource::EnvOverride);
        assert_eq!(m.matched_key, "__env_override__");
        assert_eq!(m.pricing.input_per_1m, 42.0);
        assert_eq!(m.pricing.output_per_1m, 84.0);
    }

    #[test]
    fn get_pricing_match_unknown_model() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let m = get_pricing_match("totally-unknown-model");
        assert_eq!(m.source, PricingSource::Unknown);
        assert_eq!(m.matched_key, "");
        assert_eq!(m.pricing.input_per_1m, 0.0);
        assert_eq!(m.pricing.output_per_1m, 0.0);
    }

    #[test]
    fn get_pricing_is_backwards_compatible() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let p = get_pricing("gpt-4o");
        assert_eq!(p.input_per_1m, 2.5);
        assert_eq!(p.output_per_1m, 10.0);
    }

    #[test]
    fn calculate_cost_breakdown_all_fields() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let pm = get_pricing_match("gpt-4o");
        let cb = calculate_cost_breakdown(&pm, 1000, 500, 200, 100);

        assert_eq!(cb.pricing.source, PricingSource::Builtin);
        assert_eq!(cb.pricing.matched_key, "gpt-4o");
        assert!((cb.input_cost - 1000.0 * 2.5 / 1_000_000.0).abs() < 1e-10);
        assert!((cb.output_cost - 500.0 * 10.0 / 1_000_000.0).abs() < 1e-10);
        assert!((cb.cache_read_cost - 200.0 * 2.5 * 0.1 / 1_000_000.0).abs() < 1e-10);
        assert!((cb.cache_creation_cost - 100.0 * 2.5 * 1.25 / 1_000_000.0).abs() < 1e-10);

        let expected_total =
            cb.input_cost + cb.output_cost + cb.cache_read_cost + cb.cache_creation_cost;
        assert!((cb.total_cost - expected_total).abs() < 1e-10);
    }

    #[test]
    fn calculate_cost_breakdown_unknown_returns_zero() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let pm = get_pricing_match("unknown-model");
        let cb = calculate_cost_breakdown(&pm, 50000, 10000, 2000, 1000);

        assert_eq!(cb.pricing.source, PricingSource::Unknown);
        assert_eq!(cb.pricing.matched_key, "");
        assert_eq!(cb.input_cost, 0.0);
        assert_eq!(cb.output_cost, 0.0);
        assert_eq!(cb.cache_read_cost, 0.0);
        assert_eq!(cb.cache_creation_cost, 0.0);
        assert_eq!(cb.total_cost, 0.0);
    }

    #[test]
    fn pricing_source_serialize_round_trip() {
        for (src, expected) in &[
            (PricingSource::Builtin, r#""builtin""#),
            (PricingSource::EnvOverride, r#""env_override""#),
            (PricingSource::ProviderReported, r#""provider_reported""#),
            (PricingSource::Unknown, r#""unknown""#),
            (PricingSource::Backfilled, r#""backfilled""#),
        ] {
            let json = serde_json::to_string(src).unwrap();
            assert_eq!(json, *expected);
            let deserialized: PricingSource = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, *src);
        }
    }

    #[test]
    fn cost_breakdown_serialize_round_trip() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let pm = get_pricing_match("gpt-4o");
        let cb = calculate_cost_breakdown(&pm, 100, 200, 0, 0);

        let json = serde_json::to_string(&cb).unwrap();
        let deserialized: CostBreakdown = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.pricing.matched_key, "gpt-4o");
        assert_eq!(deserialized.pricing.source, PricingSource::Builtin);
        assert!((deserialized.total_cost - cb.total_cost).abs() < 1e-12);
    }

    #[test]
    fn pricing_match_serialize_round_trip() {
        let pm = PricingMatch {
            source: PricingSource::Builtin,
            matched_key: "gpt-4o".to_string(),
            pricing: ModelPricing {
                input_per_1m: 2.5,
                output_per_1m: 10.0,
            },
        };

        let json = serde_json::to_string(&pm).unwrap();
        let deserialized: PricingMatch = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.source, PricingSource::Builtin);
        assert_eq!(deserialized.matched_key, "gpt-4o");
        assert_eq!(deserialized.pricing.input_per_1m, 2.5);
    }
}
