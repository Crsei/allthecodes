//! Runtime cost helpers backed by model pricing metadata from `cc-models`.

use allthecodes_types::message::Usage;
use allthecodes_types::models::CostBreakdown;

/// Calculate total cost in USD for a model + usage pair.
pub fn calculate_cost(model: &str, usage: &Usage) -> f64 {
    let pricing_match = allthecodes_types::models::get_pricing_match(model);
    allthecodes_types::models::calculate_cost_breakdown(
        &pricing_match,
        usage.input_tokens,
        usage.output_tokens,
        usage.cache_read_input_tokens,
        usage.cache_creation_input_tokens,
    )
    .total_cost
}

/// Calculate structured cost breakdown for a model + usage pair.
pub fn calculate_cost_breakdown(model: &str, usage: &Usage) -> CostBreakdown {
    let pricing_match = allthecodes_types::models::get_pricing_match(model);
    allthecodes_types::models::calculate_cost_breakdown(
        &pricing_match,
        usage.input_tokens,
        usage.output_tokens,
        usage.cache_read_input_tokens,
        usage.cache_creation_input_tokens,
    )
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
    fn calculate_cost_basic() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let usage = Usage {
            input_tokens: 1000,
            output_tokens: 500,
            reasoning_output_tokens: 0,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        };
        let cost = calculate_cost("gpt-4o", &usage);
        let expected = 1000.0 * 2.5 / 1_000_000.0 + 500.0 * 10.0 / 1_000_000.0;
        assert!((cost - expected).abs() < 1e-10);
    }

    #[test]
    fn calculate_cost_unknown_model_is_zero() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let usage = Usage {
            input_tokens: 1000,
            output_tokens: 500,
            reasoning_output_tokens: 0,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        };
        let cost = calculate_cost("unknown-model", &usage);

        assert_eq!(cost, 0.0);
    }

    #[test]
    fn calculate_cost_breakdown_returns_all_fields() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let usage = Usage {
            input_tokens: 1000,
            output_tokens: 500,
            reasoning_output_tokens: 0,
            cache_read_input_tokens: 200,
            cache_creation_input_tokens: 100,
        };
        let cb = calculate_cost_breakdown("gpt-4o", &usage);

        // gpt-4o: input 2.5, output 10.0 per 1M tokens
        let expected_input = 1000.0 * 2.5 / 1_000_000.0;
        let expected_output = 500.0 * 10.0 / 1_000_000.0;
        let expected_cache_read = 200.0 * 2.5 * 0.1 / 1_000_000.0;
        let expected_cache_creation = 100.0 * 2.5 * 1.25 / 1_000_000.0;
        let expected_total =
            expected_input + expected_output + expected_cache_read + expected_cache_creation;

        assert!((cb.input_cost - expected_input).abs() < 1e-10);
        assert!((cb.output_cost - expected_output).abs() < 1e-10);
        assert!((cb.cache_read_cost - expected_cache_read).abs() < 1e-10);
        assert!((cb.cache_creation_cost - expected_cache_creation).abs() < 1e-10);
        assert!((cb.total_cost - expected_total).abs() < 1e-10);
        assert_eq!(cb.pricing.matched_key, "gpt-4o");
    }

    #[test]
    fn calculate_cost_breakdown_unknown_model() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let usage = Usage {
            input_tokens: 5000,
            output_tokens: 1000,
            reasoning_output_tokens: 0,
            cache_read_input_tokens: 300,
            cache_creation_input_tokens: 200,
        };
        let cb = calculate_cost_breakdown("nonexistent", &usage);

        assert_eq!(cb.input_cost, 0.0);
        assert_eq!(cb.output_cost, 0.0);
        assert_eq!(cb.cache_read_cost, 0.0);
        assert_eq!(cb.cache_creation_cost, 0.0);
        assert_eq!(cb.total_cost, 0.0);
    }

    #[test]
    fn calculate_cost_breakdown_zero_usage() {
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let usage = Usage {
            input_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        };
        let cb = calculate_cost_breakdown("gpt-4o", &usage);

        assert_eq!(cb.input_cost, 0.0);
        assert_eq!(cb.output_cost, 0.0);
        assert_eq!(cb.cache_read_cost, 0.0);
        assert_eq!(cb.cache_creation_cost, 0.0);
        assert_eq!(cb.total_cost, 0.0);
    }

    #[test]
    fn calculate_cost_breakdown_via_cost_function() {
        // verify consistency: calculate_cost total matches breakdown total
        let _lock = lock_pricing_env();
        let _env = PricingEnvSnapshot::clear();
        let usage = Usage {
            input_tokens: 2000,
            output_tokens: 1000,
            reasoning_output_tokens: 0,
            cache_read_input_tokens: 500,
            cache_creation_input_tokens: 250,
        };
        let cost = calculate_cost("gpt-4o", &usage);
        let cb = calculate_cost_breakdown("gpt-4o", &usage);

        assert!((cost - cb.total_cost).abs() < 1e-10);
    }
}
