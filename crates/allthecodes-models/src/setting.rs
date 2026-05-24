//! Structured model display metadata.
//!
//! Corresponds to TypeScript:
//! - bootstrap/src/utils/model/model.ts (ModelSetting)
//! - bootstrap/src/utils/model/modelStrings.ts (ModelStrings)

/// Coarse model family tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelTier {
    Opus,
    Sonnet,
    Haiku,
    Unknown,
}

impl ModelTier {
    pub fn as_str(&self) -> &str {
        match self {
            ModelTier::Opus => "opus",
            ModelTier::Sonnet => "sonnet",
            ModelTier::Haiku => "haiku",
            ModelTier::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for ModelTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Structured model settings inferred from a raw model ID.
#[derive(Debug, Clone)]
pub struct ModelSetting {
    /// Raw model ID, for example `claude-sonnet-4-20250514`.
    pub model_id: String,
    /// UI display name, for example `Sonnet`.
    pub display_name: String,
    /// Coarse model family tier.
    pub tier: ModelTier,
}

impl ModelSetting {
    /// Infer tier and display name from a raw model ID.
    pub fn from_model_id(model_id: &str) -> Self {
        let lower = model_id.to_lowercase();

        let (tier, display_name) = if lower.contains("opus") {
            (ModelTier::Opus, "Opus")
        } else if lower.contains("sonnet") {
            (ModelTier::Sonnet, "Sonnet")
        } else if lower.contains("haiku") {
            (ModelTier::Haiku, "Haiku")
        } else {
            (ModelTier::Unknown, model_id)
        };

        Self {
            model_id: model_id.to_string(),
            display_name: display_name.to_string(),
            tier,
        }
    }
}

/// Model strings used for UI display and logs.
#[derive(Debug, Clone)]
pub struct ModelStrings {
    /// Full main model ID.
    pub main_model_id: String,
    /// Main model display name.
    pub main_display: String,
    /// Optional fast-mode model display name.
    pub fast_display: Option<String>,
}

impl ModelStrings {
    /// Build display strings from a model setting.
    pub fn from_setting(setting: &ModelSetting) -> Self {
        Self {
            main_model_id: setting.model_id.clone(),
            main_display: setting.display_name.clone(),
            fast_display: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_sonnet() {
        let s = ModelSetting::from_model_id("claude-sonnet-4-20250514");
        assert_eq!(s.tier, ModelTier::Sonnet);
        assert_eq!(s.display_name, "Sonnet");
    }

    #[test]
    fn detect_opus() {
        let s = ModelSetting::from_model_id("claude-opus-4-20250514");
        assert_eq!(s.tier, ModelTier::Opus);
    }

    #[test]
    fn detect_haiku() {
        let s = ModelSetting::from_model_id("claude-haiku-4-5-20251001");
        assert_eq!(s.tier, ModelTier::Haiku);
    }

    #[test]
    fn detect_unknown() {
        let s = ModelSetting::from_model_id("gpt-4o");
        assert_eq!(s.tier, ModelTier::Unknown);
        assert_eq!(s.display_name, "gpt-4o");
    }

    #[test]
    fn model_strings_from_setting() {
        let setting = ModelSetting::from_model_id("claude-opus-4-20250514");
        let strings = ModelStrings::from_setting(&setting);
        assert_eq!(strings.main_display, "Opus");
        assert!(strings.fast_display.is_none());
    }
}
