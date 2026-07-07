use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use allthecodes_config::features::{self, Feature};
use chrono::{DateTime, Utc};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchTipDismissal {
    pub plugin_id: String,
    pub dismissed_at: DateTime<Utc>,
    pub remind_after: Option<Duration>,
}

pub struct SearchTipService {
    cooldown_ms: u64,
    shown_at: HashMap<String, u64>,
    dismissal_path: Option<PathBuf>,
}

impl SearchTipService {
    pub fn new() -> Self {
        Self {
            cooldown_ms: 30_000,
            shown_at: HashMap::new(),
            dismissal_path: Some(allthecodes_config::paths::search_tip_dismissals_path()),
        }
    }

    pub fn with_cooldown_ms(mut self, cooldown_ms: u64) -> Self {
        self.cooldown_ms = cooldown_ms;
        self
    }

    pub fn with_dismissal_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.dismissal_path = Some(path.into());
        self
    }

    pub fn without_persistence(mut self) -> Self {
        self.dismissal_path = None;
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

        let dismissals = self
            .dismissal_path
            .as_deref()
            .map(load_search_tip_dismissals)
            .unwrap_or_default();
        let now = context_time(&context);
        let mut emitted_keys = HashSet::new();
        let mut suggestions = candidates
            .iter()
            .filter(|candidate| candidate.confidence >= 0.75)
            .filter_map(|candidate| {
                if candidate_is_dismissed(candidate, &dismissals, &now) {
                    return None;
                }
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

pub fn load_search_tip_dismissals(path: &Path) -> Vec<SearchTipDismissal> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };
    let Some(entries) = value.as_array() else {
        return Vec::new();
    };

    entries.iter().filter_map(parse_dismissal).collect()
}

pub fn save_search_tip_dismissal(dismissal: &SearchTipDismissal, path: &Path) {
    let mut existing = load_search_tip_dismissals(path);
    existing.retain(|entry| entry.plugin_id != dismissal.plugin_id);
    existing.push(dismissal.clone());

    let entries = existing
        .iter()
        .map(|entry| {
            serde_json::json!({
                "plugin_id": entry.plugin_id,
                "dismissed_at": entry.dismissed_at.to_rfc3339(),
                "remind_after_secs": entry.remind_after.map(|duration| duration.as_secs()),
            })
        })
        .collect::<Vec<_>>();

    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            tracing::warn!(
                path = %parent.display(),
                error = %error,
                "failed to create search tip dismissal directory"
            );
            return;
        }
    }

    let json = serde_json::to_string_pretty(&entries).unwrap_or_else(|_| "[]".to_string());
    if let Err(error) = std::fs::write(path, json) {
        tracing::warn!(
            path = %path.display(),
            error = %error,
            "failed to write search tip dismissals"
        );
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

fn context_time(context: &SearchTipContext) -> DateTime<Utc> {
    let millis = i64::try_from(context.now_ms).unwrap_or(i64::MAX);
    DateTime::<Utc>::from_timestamp_millis(millis).unwrap_or_else(Utc::now)
}

fn parse_dismissal(entry: &serde_json::Value) -> Option<SearchTipDismissal> {
    let object = entry.as_object()?;
    let plugin_id = object
        .get("plugin_id")
        .or_else(|| object.get("id"))?
        .as_str()?
        .to_string();
    let dismissed_at = DateTime::parse_from_rfc3339(object.get("dismissed_at")?.as_str()?)
        .ok()?
        .with_timezone(&Utc);
    let remind_after = object
        .get("remind_after_secs")
        .and_then(|value| value.as_u64())
        .map(Duration::from_secs);
    Some(SearchTipDismissal {
        plugin_id,
        dismissed_at,
        remind_after,
    })
}

fn candidate_is_dismissed(
    candidate: &SearchTipCandidate,
    dismissals: &[SearchTipDismissal],
    now: &DateTime<Utc>,
) -> bool {
    candidate_dismissal_keys(candidate)
        .iter()
        .any(|key| is_search_tip_dismissed_at(key, dismissals, now))
}

fn candidate_dismissal_keys(candidate: &SearchTipCandidate) -> Vec<String> {
    let stable_key = format!(
        "{}::{}",
        kind_storage_label(candidate.kind),
        candidate.id.trim()
    );
    if stable_key == candidate.id {
        vec![stable_key]
    } else {
        vec![stable_key, candidate.id.clone()]
    }
}

fn is_search_tip_dismissed_at(
    plugin_id: &str,
    dismissals: &[SearchTipDismissal],
    now: &DateTime<Utc>,
) -> bool {
    dismissals.iter().any(|dismissal| {
        if dismissal.plugin_id != plugin_id {
            return false;
        }
        match dismissal.remind_after {
            None => true,
            Some(duration) => chrono::Duration::from_std(duration)
                .map(|duration| *now < dismissal.dismissed_at + duration)
                .unwrap_or(false),
        }
    })
}

fn kind_storage_label(kind: SearchTipKind) -> &'static str {
    match kind {
        SearchTipKind::Skill => "skill",
        SearchTipKind::Mcp => "mcp",
        SearchTipKind::Plugin => "plugin",
    }
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

    fn at_ms(ms: i64) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms).unwrap()
    }

    #[test]
    #[serial]
    fn disabled_gates_suppress_tips() {
        let _features = FeatureGuard::set(FeatureFlags::all_disabled());
        let mut service = SearchTipService::new().without_persistence();

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
        let mut service = SearchTipService::new().without_persistence();

        assert!(service.try_generate(context(0), &[]).is_none());
    }

    #[test]
    #[serial]
    fn high_confidence_candidate_becomes_advisory_prompt() {
        let mut flags = FeatureFlags::all_disabled();
        flags.kairos = true;
        flags.proactive = true;
        let _features = FeatureGuard::set(flags);
        let mut service = SearchTipService::new().without_persistence();

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
        let mut service = SearchTipService::new()
            .without_persistence()
            .with_cooldown_ms(100);
        let candidate = skill_candidate();

        let first = service
            .try_generate(context(1_000), &[candidate.clone(), candidate.clone()])
            .expect("first tip");
        assert_eq!(first.len(), 1);

        assert!(service
            .try_generate(context(1_050), std::slice::from_ref(&candidate))
            .is_none());

        assert!(
            service
                .try_generate(context(1_200), &[candidate])
                .expect("cooldown elapsed")
                .len()
                == 1
        );
    }

    #[test]
    #[serial]
    fn persistent_dismissal_file_uses_lsp_compatible_shape_and_replaces_existing_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("search-tip-dismissals.json");
        let key = "skill::rust-review".to_string();

        save_search_tip_dismissal(
            &SearchTipDismissal {
                plugin_id: key.clone(),
                dismissed_at: at_ms(1_000),
                remind_after: None,
            },
            &path,
        );
        save_search_tip_dismissal(
            &SearchTipDismissal {
                plugin_id: key,
                dismissed_at: at_ms(2_000),
                remind_after: Some(std::time::Duration::from_secs(3600)),
            },
            &path,
        );

        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 1);
        assert_eq!(value[0]["plugin_id"], "skill::rust-review");
        assert_eq!(value[0]["dismissed_at"], at_ms(2_000).to_rfc3339());
        assert_eq!(value[0]["remind_after_secs"], 3600);

        let loaded = load_search_tip_dismissals(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].plugin_id, "skill::rust-review");
        assert_eq!(
            loaded[0].remind_after,
            Some(std::time::Duration::from_secs(3600))
        );
    }

    #[test]
    #[serial]
    fn persistent_dismissal_suppresses_candidate_until_remind_after_expires() {
        let mut flags = FeatureFlags::all_disabled();
        flags.proactive = true;
        let _features = FeatureGuard::set(flags);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("search-tip-dismissals.json");
        save_search_tip_dismissal(
            &SearchTipDismissal {
                plugin_id: "skill::rust-review".to_string(),
                dismissed_at: at_ms(1_000),
                remind_after: Some(std::time::Duration::from_secs(60)),
            },
            &path,
        );
        let mut service = SearchTipService::new()
            .with_dismissal_path(path)
            .with_cooldown_ms(0);
        let candidate = skill_candidate();

        assert!(service
            .try_generate(context(30_000), std::slice::from_ref(&candidate))
            .is_none());

        let tips = service
            .try_generate(context(70_000), &[candidate])
            .expect("remind-later dismissal should expire");
        assert_eq!(tips.len(), 1);
        assert!(tips[0].text.contains("/skills rust-review"));
    }
}
