//! Model token pricing built-in table plus environment variable overrides.

/// Price per 1 million tokens in USD.
#[derive(Debug, Clone, Copy)]
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

/// Look up pricing for a model.
pub fn get_pricing(model: &str) -> ModelPricing {
    if let Some(pricing) = pricing_from_env() {
        return pricing;
    }
    if let Some(pricing) = pricing_from_table(model) {
        return pricing;
    }
    ModelPricing::ZERO
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

fn pricing_from_table(model: &str) -> Option<ModelPricing> {
    let lower = model.to_lowercase();

    for &(prefix, input, output) in BUILTIN_PRICING {
        if lower == prefix {
            return Some(ModelPricing {
                input_per_1m: input,
                output_per_1m: output,
            });
        }
    }

    for &(prefix, input, output) in BUILTIN_PRICING {
        if lower.starts_with(prefix) {
            return Some(ModelPricing {
                input_per_1m: input,
                output_per_1m: output,
            });
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
}
