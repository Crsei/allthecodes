use std::collections::HashMap;
use std::sync::LazyLock;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use allthecodes_config::features::{self, Feature};

use crate::search_tips::{SearchTipCandidate, SearchTipKind};

static TURN_ZERO: LazyLock<Mutex<HashMap<String, SkillPrefetchResult>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillPrefetchContext {
    pub session_id: String,
    pub query: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillPrefetchSkill {
    pub name: String,
    pub description: String,
    pub source: String,
    pub when_to_use: Option<String>,
    pub argument_hint: Option<String>,
    pub argument_names: Vec<String>,
    pub paths: Vec<String>,
    pub assets: Vec<String>,
    pub entry_docs: Vec<String>,
    pub dependencies: Vec<String>,
    pub prompt_body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillPrefetchResult {
    pub session_id: String,
    pub query: String,
    pub skills: Vec<SkillPrefetchSkill>,
    pub remote_state: String,
}

#[derive(Debug, Clone)]
pub struct SkillPrefetchHandle {
    result: SkillPrefetchResult,
}

pub fn start_skill_discovery_prefetch(context: SkillPrefetchContext) -> SkillPrefetchHandle {
    if !features::enabled(Feature::ExperimentalSkillSearch) {
        return SkillPrefetchHandle {
            result: SkillPrefetchResult {
                session_id: context.session_id,
                query: context.query,
                skills: Vec::new(),
                remote_state: "not_configured".to_string(),
            },
        };
    }

    let skills = local_skill_metadata(&context.query);
    let remote_state = if features::enabled(Feature::RemoteUrlDiscovery) {
        "deferred"
    } else {
        "feature_disabled"
    };
    SkillPrefetchHandle {
        result: SkillPrefetchResult {
            session_id: context.session_id,
            query: context.query,
            skills,
            remote_state: remote_state.to_string(),
        },
    }
}

pub fn collect_skill_discovery_prefetch(handle: SkillPrefetchHandle) -> SkillPrefetchResult {
    let result = handle.result;
    if result.remote_state != "not_configured" {
        TURN_ZERO
            .lock()
            .insert(result.session_id.clone(), result.clone());
    }
    result
}

pub fn get_turn_zero_skill_discovery(session_id: &str) -> Option<SkillPrefetchResult> {
    TURN_ZERO.lock().get(session_id).cloned()
}

pub fn ensure_turn_zero_skill_discovery(session_id: &str, query: &str) -> SkillPrefetchResult {
    get_turn_zero_skill_discovery(session_id).unwrap_or_else(|| {
        collect_skill_discovery_prefetch(start_skill_discovery_prefetch(SkillPrefetchContext {
            session_id: session_id.to_string(),
            query: query.to_string(),
        }))
    })
}

pub fn candidates_from_prefetch(result: &SkillPrefetchResult) -> Vec<SearchTipCandidate> {
    if result.remote_state == "not_configured" || result.skills.is_empty() {
        return Vec::new();
    }

    result
        .skills
        .iter()
        .map(|skill| {
            let description = match skill.when_to_use.as_deref() {
                Some(when_to_use) if !when_to_use.trim().is_empty() => {
                    format!("{} ({when_to_use})", skill.description)
                }
                _ => skill.description.clone(),
            };
            SearchTipCandidate {
                kind: SearchTipKind::Skill,
                id: skill.name.clone(),
                label: skill.name.clone(),
                description,
                confidence: 0.82,
                next_action: format!("/skills {}", skill.name),
            }
        })
        .collect()
}

fn local_skill_metadata(query: &str) -> Vec<SkillPrefetchSkill> {
    let query = query.trim().to_ascii_lowercase();
    let mut seen = std::collections::HashSet::new();
    let mut skills = allthecodes_skills::get_all_skills()
        .into_iter()
        .filter(|skill| {
            query.is_empty()
                || skill.name.to_ascii_lowercase().contains(&query)
                || skill
                    .frontmatter
                    .description
                    .to_ascii_lowercase()
                    .contains(&query)
                || skill
                    .frontmatter
                    .when_to_use
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains(&query)
        })
        .filter_map(|skill| {
            if !seen.insert(skill.name.clone()) {
                return None;
            }
            let source = source_label(&skill.source);
            let frontmatter = skill.frontmatter;
            Some(SkillPrefetchSkill {
                name: skill.name,
                description: frontmatter.description,
                source,
                when_to_use: frontmatter.when_to_use,
                argument_hint: frontmatter.argument_hint,
                argument_names: frontmatter.argument_names,
                paths: frontmatter.paths,
                assets: frontmatter.assets,
                entry_docs: frontmatter.entry_docs,
                dependencies: frontmatter
                    .dependencies
                    .iter()
                    .map(|dependency| dependency.label())
                    .collect(),
                prompt_body: skill.prompt_body,
            })
        })
        .collect::<Vec<_>>();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills.truncate(25);
    skills
}

fn source_label(source: &allthecodes_skills::SkillSource) -> String {
    match source {
        allthecodes_skills::SkillSource::Bundled => "bundled".to_string(),
        allthecodes_skills::SkillSource::User => "user".to_string(),
        allthecodes_skills::SkillSource::Project => "project".to_string(),
        allthecodes_skills::SkillSource::Plugin(name) => format!("plugin:{name}"),
        allthecodes_skills::SkillSource::Mcp(name) => format!("mcp:{name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_config::features::{self, FeatureFlags};
    use allthecodes_skills::{SkillDefinition, SkillDependency, SkillFrontmatter, SkillSource};
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

    struct SkillRegistryGuard;

    impl SkillRegistryGuard {
        fn new(skills: Vec<SkillDefinition>) -> Self {
            allthecodes_skills::clear_skills();
            for skill in skills {
                allthecodes_skills::register_skill(skill);
            }
            Self
        }
    }

    impl Drop for SkillRegistryGuard {
        fn drop(&mut self) {
            allthecodes_skills::clear_skills();
            TURN_ZERO.lock().clear();
        }
    }

    fn make_skill(name: &str, description: &str) -> SkillDefinition {
        SkillDefinition {
            name: name.to_string(),
            source: SkillSource::User,
            base_dir: None,
            frontmatter: SkillFrontmatter {
                name: Some(name.to_string()),
                description: description.to_string(),
                when_to_use: Some(format!("Use {name} locally")),
                argument_hint: Some("TARGET".to_string()),
                argument_names: vec!["TARGET".to_string()],
                dependencies: vec![SkillDependency::new(
                    "base-helper",
                    Some(">=1.0.0".to_string()),
                )],
                paths: vec!["src/**/*.rs".to_string()],
                assets: vec!["references/checklist.md".to_string()],
                entry_docs: vec!["README.md".to_string()],
                ..Default::default()
            },
            prompt_body: "local prompt body with $ARGUMENTS".to_string(),
        }
    }

    fn context(session_id: &str, query: &str) -> SkillPrefetchContext {
        SkillPrefetchContext {
            session_id: session_id.to_string(),
            query: query.to_string(),
        }
    }

    #[test]
    #[serial]
    fn disabled_gate_returns_empty_not_configured_result() {
        let _features = FeatureGuard::set(FeatureFlags::all_disabled());
        let _skills = SkillRegistryGuard::new(vec![make_skill("rust-review", "Review Rust")]);

        let result = collect_skill_discovery_prefetch(start_skill_discovery_prefetch(context(
            "session-disabled",
            "rust",
        )));

        assert!(result.skills.is_empty());
        assert_eq!(result.remote_state, "not_configured");
        assert!(get_turn_zero_skill_discovery("session-disabled").is_none());
    }

    #[test]
    #[serial]
    fn prefetch_collects_local_skill_metadata_only() {
        let mut flags = FeatureFlags::all_disabled();
        flags.experimental_skill_search = true;
        flags.remote_url_discovery = true;
        let _features = FeatureGuard::set(flags);
        let _skills = SkillRegistryGuard::new(vec![make_skill(
            "rust-review",
            "Review Rust code without remote fetch",
        )]);

        let result = collect_skill_discovery_prefetch(start_skill_discovery_prefetch(context(
            "session-prefetch",
            "rust",
        )));

        assert_eq!(result.skills.len(), 1);
        assert_eq!(result.skills[0].name, "rust-review");
        assert_eq!(result.skills[0].argument_hint.as_deref(), Some("TARGET"));
        assert_eq!(result.skills[0].argument_names, vec!["TARGET"]);
        assert_eq!(result.skills[0].paths, vec!["src/**/*.rs"]);
        assert_eq!(result.skills[0].assets, vec!["references/checklist.md"]);
        assert_eq!(result.skills[0].entry_docs, vec!["README.md"]);
        assert_eq!(result.skills[0].dependencies, vec!["base-helper >=1.0.0"]);
        assert_eq!(
            result.skills[0].prompt_body,
            "local prompt body with $ARGUMENTS"
        );
        assert_eq!(result.remote_state, "deferred");
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("http://"));
        assert!(!json.contains("https://"));
    }

    #[test]
    #[serial]
    fn remote_url_discovery_gate_controls_remote_state_without_blocking_local_prefetch() {
        let mut flags = FeatureFlags::all_disabled();
        flags.experimental_skill_search = true;
        flags.remote_url_discovery = false;
        let _features = FeatureGuard::set(flags);
        let _skills = SkillRegistryGuard::new(vec![make_skill(
            "rust-review",
            "Review Rust code without remote fetch",
        )]);

        let result = collect_skill_discovery_prefetch(start_skill_discovery_prefetch(context(
            "session-remote-disabled",
            "rust",
        )));

        assert_eq!(result.skills.len(), 1);
        assert_eq!(result.remote_state, "feature_disabled");
        assert_eq!(candidates_from_prefetch(&result).len(), 1);
    }

    #[test]
    #[serial]
    fn collection_stores_turn_zero_result_by_session() {
        let mut flags = FeatureFlags::all_disabled();
        flags.experimental_skill_search = true;
        let _features = FeatureGuard::set(flags);
        let _skills = SkillRegistryGuard::new(vec![make_skill("debug-helper", "Debug failures")]);

        let result = collect_skill_discovery_prefetch(start_skill_discovery_prefetch(context(
            "session-turn-zero",
            "debug",
        )));

        assert_eq!(
            get_turn_zero_skill_discovery("session-turn-zero"),
            Some(result)
        );
    }

    #[test]
    fn prefetch_result_becomes_skill_search_tip_candidates() {
        let result = SkillPrefetchResult {
            session_id: "session-candidate".to_string(),
            query: "rust".to_string(),
            skills: vec![SkillPrefetchSkill {
                name: "rust-review".to_string(),
                description: "Review Rust code".to_string(),
                source: "user".to_string(),
                when_to_use: Some("Use rust-review locally".to_string()),
                argument_hint: None,
                argument_names: Vec::new(),
                paths: Vec::new(),
                assets: Vec::new(),
                entry_docs: Vec::new(),
                dependencies: Vec::new(),
                prompt_body: "Review Rust code".to_string(),
            }],
            remote_state: "deferred".to_string(),
        };

        let candidates = candidates_from_prefetch(&result);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, "rust-review");
        assert_eq!(candidates[0].next_action, "/skills rust-review");
        assert!(candidates[0].description.contains("Review Rust code"));
    }
}
