//! Explicit feature gates for context compaction lanes.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionFeatureGates {
    pub auto_compact: bool,
    pub reactive_compact: bool,
    pub session_memory_compact: bool,
    pub partial_compact: bool,
}

impl Default for CompactionFeatureGates {
    fn default() -> Self {
        Self {
            auto_compact: true,
            reactive_compact: true,
            session_memory_compact: true,
            partial_compact: true,
        }
    }
}

impl CompactionFeatureGates {
    pub fn from_env() -> Self {
        Self::from_env_iter(std::env::vars())
    }

    pub fn from_env_iter(iter: impl IntoIterator<Item = (String, String)>) -> Self {
        let env = iter.into_iter().collect::<HashMap<_, _>>();
        let enabled_by_default = |name: &str, legacy_name: &str| -> bool {
            env.get(name)
                .or_else(|| env.get(legacy_name))
                .map(|value| !is_falsey(value))
                .unwrap_or(true)
        };
        let disabled = |name: &str, legacy_name: &str| -> bool {
            env.get(name)
                .or_else(|| env.get(legacy_name))
                .map(|value| is_truthy(value))
                .unwrap_or(false)
        };

        Self {
            auto_compact: enabled_by_default("ALLTHECODES_AUTO_COMPACT", "CC_RUST_AUTO_COMPACT")
                && !disabled(
                    "ALLTHECODES_DISABLE_AUTO_COMPACT",
                    "CC_RUST_DISABLE_AUTO_COMPACT",
                ),
            reactive_compact: enabled_by_default(
                "ALLTHECODES_REACTIVE_COMPACT",
                "CC_RUST_REACTIVE_COMPACT",
            ),
            session_memory_compact: enabled_by_default(
                "ALLTHECODES_SESSION_MEMORY_COMPACT",
                "CC_RUST_SESSION_MEMORY_COMPACT",
            ),
            partial_compact: enabled_by_default(
                "ALLTHECODES_PARTIAL_COMPACT",
                "CC_RUST_PARTIAL_COMPACT",
            ),
        }
    }
}

fn is_falsey(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "no" | "off"
    )
}

fn is_truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gates(pairs: &[(&str, &str)]) -> CompactionFeatureGates {
        CompactionFeatureGates::from_env_iter(
            pairs
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string())),
        )
    }

    #[test]
    fn defaults_enable_all_compaction_lanes() {
        assert_eq!(gates(&[]), CompactionFeatureGates::default());
    }

    #[test]
    fn explicit_false_values_disable_individual_lanes() {
        let gates = gates(&[
            ("CC_RUST_AUTO_COMPACT", "0"),
            ("CC_RUST_REACTIVE_COMPACT", "false"),
            ("CC_RUST_SESSION_MEMORY_COMPACT", "off"),
            ("CC_RUST_PARTIAL_COMPACT", "no"),
        ]);

        assert!(!gates.auto_compact);
        assert!(!gates.reactive_compact);
        assert!(!gates.session_memory_compact);
        assert!(!gates.partial_compact);
    }

    #[test]
    fn legacy_disable_auto_compact_flag_is_honored() {
        let gates = gates(&[("CC_RUST_DISABLE_AUTO_COMPACT", "1")]);

        assert!(!gates.auto_compact);
        assert!(gates.reactive_compact);
        assert!(gates.session_memory_compact);
        assert!(gates.partial_compact);
    }
}
