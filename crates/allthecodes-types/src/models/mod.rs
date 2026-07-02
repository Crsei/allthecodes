//! Shared model metadata for allthecodes.

pub mod aliases;
pub mod mapping;
pub mod pricing;
pub mod setting;

pub use aliases::{
    alias_for_model, default_fallback_model_id, default_fast_model_id, default_model_id,
    is_removed_legacy_model_alias, removed_legacy_model_alias_error,
    replacement_for_removed_legacy_alias, resolve_model_alias, ModelAlias,
    DEFAULT_FALLBACK_MODEL_ALIAS, DEFAULT_FAST_MODEL_ALIAS, DEFAULT_MODEL_ALIAS, FOTA_MODEL_ID,
    MODEL_ALIASES, MOTA_MODEL_ID, REMOVED_LEGACY_MODEL_ALIASES, SOTA_MODEL_ID,
};
pub use mapping::{to_bedrock_model_id, to_vertex_model_id, ModelConfig, CLAUDE_MODELS};
pub use pricing::{
    calculate_cost_breakdown, cost_from_counts, get_pricing, get_pricing_match, CostBreakdown,
    ModelPricing, PricingMatch, PricingSource,
};
pub use setting::{ModelSetting, ModelStrings, ModelTier};
