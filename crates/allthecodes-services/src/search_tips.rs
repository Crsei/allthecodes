use std::collections::{HashMap, HashSet};

use allthecodes_config::features::{self, Feature};

use crate::prompt_suggestion::{PromptSuggestion, SuggestionCategory};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearchTipKind {
    Skill,
    Mcp,
    Plugin,
}

#[derive(Debug, Clone)]
pub struct SearchTipCandidate {
    pub kind: SearchTipKind,
    pub id: String,
    pub label: String,
    pub description: String,
    pub confidence: f32,
    pub next_action: String,
}

#[derive(Debug, Clone)]
pub struct SearchTipContext {
    pub session_id: String,
    pub now_ms: u64,
}

pub struct SearchTipService {
    cooldown_ms: u64,
    shown_at: HashMap<String, u64>,
}

impl SearchTipService {
    pub fn new() -> Self {
        Self {
            cooldown_ms: 30_000,
            shown_at: HashMap::new(),
        }
    }

    pub fn with_cooldown_ms(mut self, cooldown_ms: u64) -> Self {
        self.cooldown_ms = cooldown_ms;
        self
    }

    pub fn try_generate(
        &mut self,
        context: SearchTipContext,
        candidates: &[SearchTipCandidate],
    ) -> Option<Vec<PromptSuggestion>> {
        if !tips_enabled() || candidates.is_empty() {
            return None;
        }

        let mut emitted_keys = HashSet::new();
        let mut suggestions = candidates
            .iter()
            .filter(|candidate| candidate.confidence >= 0.75)
            .filter_map(|candidate| {
                let key = candidate_key(&context, candidate);
                if !emitted_keys.insert(key.clone()) {
                    return None;
                }
                if let Some(last) = self.shown_at.get(&key) {
                    if context.now_ms.saturating_sub(*last) < self.cooldown_ms {
                        return None;
                    }
                }
                self.shown_at.insert(key, context.now_ms);
                Some(PromptSuggestion {
                    text: format!(
                        "Consider {} search result '{}': {}. Next: {}",
                        kind_label(candidate.kind),
                        candidate.label,
                        candidate.description,
                        candidate.next_action
                    ),
                    confidence: candidate.confidence.clamp(0.0, 1.0),
                    category: SuggestionCategory::Action,
                })
            })
            .collect::<Vec<_>>();

        if suggestions.is_empty() {
            None
        } else {
            suggestions.sort_by(|a, b| {
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            Some(suggestions)
        }
    }
}

impl Default for SearchTipService {
    fn default() -> Self {
        Self::new()
    }
}

fn tips_enabled() -> bool {
    features::enabled(Feature::Kairos) || features::enabled(Feature::Proactive)
}

fn candidate_key(context: &SearchTipContext, candidate: &SearchTipCandidate) -> String {
    format!(
        "{}::{:?}::{}",
        context.session_id, candidate.kind, candidate.id
    )
}

fn kind_label(kind: SearchTipKind) -> &'static str {
    match kind {
        SearchTipKind::Skill => "skill",
        SearchTipKind::Mcp => "MCP",
        SearchTipKind::Plugin => "plugin",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_config::features::{self, FeatureFlags};
    use serial_test::serial;

    struct FeatureGuard;

    impl FeatureGuard {
        fn set(flags: FeatureFlags) -> Self {
            features::set_runtime_override(flags);
            Self
        }
    }

    impl Drop for FeatureGuard {
        fn drop(&mut self) {
            features::clear_runtime_override();
        }
    }

    fn context(now_ms: u64) -> SearchTipContext {
        SearchTipContext {
            session_id: "session-1".to_string(),
            now_ms,
        }
    }

    fn skill_candidate() -> SearchTipCandidate {
        SearchTipCandidate {
            kind: SearchTipKind::Skill,
            id: "rust-review".to_string(),
            label: "rust-review".to_string(),
            description: "Review Rust code".to_string(),
            confidence: 0.91,
            next_action: "/skills rust-review".to_string(),
        }
    }

    #[test]
    #[serial]
    fn disabled_gates_suppress_tips() {
        let _features = FeatureGuard::set(FeatureFlags::all_disabled());
        let mut service = SearchTipService::new();

        assert!(service
            .try_generate(context(0), &[skill_candidate()])
            .is_none());
    }

    #[test]
    #[serial]
    fn no_candidates_returns_none() {
        let mut flags = FeatureFlags::all_disabled();
        flags.proactive = true;
        let _features = FeatureGuard::set(flags);
        let mut service = SearchTipService::new();

        assert!(service.try_generate(context(0), &[]).is_none());
    }

    #[test]
    #[serial]
    fn high_confidence_candidate_becomes_advisory_prompt() {
        let mut flags = FeatureFlags::all_disabled();
        flags.kairos = true;
        flags.proactive = true;
        let _features = FeatureGuard::set(flags);
        let mut service = SearchTipService::new();

        let tips = service
            .try_generate(context(1_000), &[skill_candidate()])
            .expect("tip should be generated");

        assert_eq!(tips.len(), 1);
        assert!(tips[0].text.contains("Consider"));
        assert!(tips[0].text.contains("/skills rust-review"));
        assert!(tips[0].confidence >= 0.8);
    }

    #[test]
    #[serial]
    fn duplicate_candidates_and_cooldown_are_suppressed() {
        let mut flags = FeatureFlags::all_disabled();
        flags.proactive = true;
        let _features = FeatureGuard::set(flags);
        let mut service = SearchTipService::new().with_cooldown_ms(100);
        let candidate = skill_candidate();

        let first = service
            .try_generate(context(1_000), &[candidate.clone(), candidate.clone()])
            .expect("first tip");
        assert_eq!(first.len(), 1);

        assert!(service
            .try_generate(context(1_050), &[candidate.clone()])
            .is_none());

        assert!(
            service
                .try_generate(context(1_200), &[candidate])
                .expect("cooldown elapsed")
                .len()
                == 1
        );
    }
}
