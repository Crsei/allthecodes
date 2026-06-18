//! Runtime cost helpers backed by model pricing metadata from `cc-models`.

use allthecodes_types::message::Usage;

/// Calculate total cost in USD for a model + usage pair.
pub fn calculate_cost(model: &str, usage: &Usage) -> f64 {
    allthecodes_types::models::get_pricing(model).cost_from_counts(
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
}
