//! Feature gate system for KAIROS and related features.
//!
//! Each feature is controlled by an environment variable (`FEATURE_*`), with
//! Agent Teams also honoring `ALLTHECODES_EXPERIMENTAL_AGENT_TEAMS` and the
//! upstream compatibility alias.
//! Dependency rules enforce that child features require their parent:
//! - `kairos_brief`, `kairos_channels`, `kairos_push_notification`,
//!   `kairos_github_webhooks` all require `kairos`.
//! - `proactive` can be standalone OR is implied when `kairos` is enabled.
//! - Full-build Phase 5 tool gates default to enabled and can be explicitly
//!   disabled with `0`, `false`, or `no`.
//!
//! A global singleton [`FLAGS`] is lazily initialised from real env vars.
//! Use [`enabled`] for quick queries from anywhere in the crate.

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

use allthecodes_types::kairos::{
    KairosFeatureProfile, KairosProfileResolution, KairosProfileSources, KairosValueSource,
};

use crate::settings::{EffectiveSettings, LoadedSettings};

// ---------------------------------------------------------------------------
// Feature enum
// ---------------------------------------------------------------------------

/// Individual feature variants used for runtime queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    Kairos,
    KairosBrief,
    KairosChannels,
    KairosPushNotification,
    KairosGithubWebhooks,
    Proactive,
    McpSkills,
    ExperimentalSkillSearch,
    RemoteUrlDiscovery,
    TeamMemory,
    SubagentDashboard,
    AgentTeams,
    Coordinator,
    WorkflowScripts,
    PushNotificationRemoteBridge,
    GoalTools,
    MultiAgentV2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureDescriptor {
    pub feature: Feature,
    pub env_var: &'static str,
    pub label: &'static str,
    pub description: &'static str,
}

const FEATURE_DESCRIPTORS: &[FeatureDescriptor] = &[
    FeatureDescriptor {
        feature: Feature::Kairos,
        env_var: "FEATURE_KAIROS",
        label: "kairos",
        description: "assistant mode and KAIROS daemon gate",
    },
    FeatureDescriptor {
        feature: Feature::KairosBrief,
        env_var: "FEATURE_KAIROS_BRIEF",
        label: "kairos_brief",
        description: "BriefTool-only response mode",
    },
    FeatureDescriptor {
        feature: Feature::KairosChannels,
        env_var: "FEATURE_KAIROS_CHANNELS",
        label: "kairos_channels",
        description: "connected assistant channels",
    },
    FeatureDescriptor {
        feature: Feature::KairosPushNotification,
        env_var: "FEATURE_KAIROS_PUSH_NOTIFICATION",
        label: "kairos_push_notification",
        description: "assistant push notification commands",
    },
    FeatureDescriptor {
        feature: Feature::KairosGithubWebhooks,
        env_var: "FEATURE_KAIROS_GITHUB_WEBHOOKS",
        label: "kairos_github_webhooks",
        description: "KAIROS GitHub webhook integration",
    },
    FeatureDescriptor {
        feature: Feature::Proactive,
        env_var: "FEATURE_PROACTIVE",
        label: "proactive",
        description: "proactive tick and sleep tooling",
    },
    FeatureDescriptor {
        feature: Feature::McpSkills,
        env_var: "FEATURE_MCP_SKILLS",
        label: "mcp_skills",
        description: "MCP skill:// resource ingestion",
    },
    FeatureDescriptor {
        feature: Feature::ExperimentalSkillSearch,
        env_var: "FEATURE_EXPERIMENTAL_SKILL_SEARCH",
        label: "experimental_skill_search",
        description: "local skill search prefetch and turn-zero discovery scaffolding",
    },
    FeatureDescriptor {
        feature: Feature::RemoteUrlDiscovery,
        env_var: "FEATURE_REMOTE_URL_DISCOVERY",
        label: "remote_url_discovery",
        description: "remote skill/plugin/MCP URL discovery",
    },
    FeatureDescriptor {
        feature: Feature::TeamMemory,
        env_var: "FEATURE_TEAMMEM",
        label: "team_memory",
        description: "team memory scope and daemon proxy",
    },
    FeatureDescriptor {
        feature: Feature::SubagentDashboard,
        env_var: "FEATURE_SUBAGENT_DASHBOARD",
        label: "subagent_dashboard",
        description: "subagent dashboard companion",
    },
    FeatureDescriptor {
        feature: Feature::AgentTeams,
        env_var: "ALLTHECODES_EXPERIMENTAL_AGENT_TEAMS",
        label: "agent_teams",
        description: "beta Agent Teams slash command/tooling",
    },
    FeatureDescriptor {
        feature: Feature::Coordinator,
        env_var: "ALLTHECODES_COORDINATOR_MODE",
        label: "coordinator",
        description: "beta coordinator mode prompt and orchestration gate",
    },
    FeatureDescriptor {
        feature: Feature::WorkflowScripts,
        env_var: "ALLTHECODES_WORKFLOW_SCRIPTS",
        label: "workflow_scripts",
        description: "workflow tool scripts and durable workflow state tools",
    },
    FeatureDescriptor {
        feature: Feature::PushNotificationRemoteBridge,
        env_var: "ALLTHECODES_PUSH_NOTIFICATION_REMOTE_BRIDGE",
        label: "push_notification_remote_bridge",
        description: "remote webhook bridge for push notification tooling",
    },
    FeatureDescriptor {
        feature: Feature::GoalTools,
        env_var: "ALLTHECODES_GOAL_TOOLS",
        label: "goal_tools",
        description: "goal lifecycle tools and runtime accounting",
    },
    FeatureDescriptor {
        feature: Feature::MultiAgentV2,
        env_var: "ALLTHECODES_MULTI_AGENT_V2",
        label: "multi_agent_v2",
        description: "multi-agent v2 list/followup/wait/close tool aliases",
    },
];

pub fn feature_descriptors() -> &'static [FeatureDescriptor] {
    FEATURE_DESCRIPTORS
}

// ---------------------------------------------------------------------------
// FeatureFlags
// ---------------------------------------------------------------------------

/// Resolved set of feature flags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeatureFlags {
    pub kairos: bool,
    pub kairos_brief: bool,
    pub kairos_channels: bool,
    pub kairos_push_notification: bool,
    pub kairos_github_webhooks: bool,
    pub proactive: bool,
    pub mcp_skills: bool,
    pub experimental_skill_search: bool,
    pub remote_url_discovery: bool,
    pub team_memory: bool,
    pub subagent_dashboard: bool,
    pub agent_teams: bool,
    pub coordinator: bool,
    pub workflow_scripts: bool,
    pub push_notification_remote_bridge: bool,
    pub goal_tools: bool,
    pub multi_agent_v2: bool,
}

impl FeatureFlags {
    /// Build flags from real environment variables.
    pub fn from_env() -> Self {
        Self::from_env_iter(std::env::vars())
    }

    /// Resolve feature flags from typed settings, preserving environment
    /// variables as the higher-priority compatibility override.
    pub fn from_settings(settings: &EffectiveSettings) -> Self {
        Self::from_profile_and_env_iter(&settings.kairos, std::env::vars())
    }

    pub fn from_profile_and_env_iter(
        profile: &KairosFeatureProfile,
        iter: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        let mut env: HashMap<String, String> = iter.into_iter().collect();
        for (key, value) in [
            ("FEATURE_KAIROS", profile.enabled),
            ("FEATURE_KAIROS_BRIEF", profile.brief),
            ("FEATURE_KAIROS_CHANNELS", profile.channels),
            (
                "FEATURE_KAIROS_PUSH_NOTIFICATION",
                profile.push_notifications,
            ),
            ("FEATURE_KAIROS_GITHUB_WEBHOOKS", profile.github_webhooks),
            ("FEATURE_PROACTIVE", profile.proactive),
        ] {
            env.entry(key.to_string())
                .or_insert_with(|| if value { "1" } else { "0" }.to_string());
        }
        Self::from_env_iter(env)
    }

    /// Build flags from an iterator of `(key, value)` pairs.
    ///
    /// A variable is considered *enabled* when its value, after trimming and
    /// lowercasing, is `"1"` or `"true"`.  Anything else (including absence)
    /// is treated as disabled.
    pub fn from_env_iter(iter: impl IntoIterator<Item = (String, String)>) -> Self {
        let env: HashMap<String, String> = iter.into_iter().collect();

        let read = |key: &str| -> bool {
            env.get(key)
                .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
                .unwrap_or(false)
        };
        let read_optional = |key: &str| -> Option<bool> {
            env.get(key)
                .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        };
        let read_default_enabled = |key: &str| -> bool {
            env.get(key)
                .map(|v| !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "false" | "no"))
                .unwrap_or(true)
        };

        let kairos = read("FEATURE_KAIROS");
        let team_memory = read("FEATURE_TEAMMEM");
        let mcp_skills = read("FEATURE_MCP_SKILLS");
        let experimental_skill_search = read("FEATURE_EXPERIMENTAL_SKILL_SEARCH");
        let remote_url_discovery = read("FEATURE_REMOTE_URL_DISCOVERY");
        let subagent_dashboard = read("FEATURE_SUBAGENT_DASHBOARD");
        let agent_teams = read_optional("FEATURE_AGENT_TEAMS")
            .or_else(|| read_optional("ALLTHECODES_EXPERIMENTAL_AGENT_TEAMS"))
            .or_else(|| read_optional("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"))
            .unwrap_or(false);
        let coordinator = read_optional("ALLTHECODES_COORDINATOR_MODE")
            .or_else(|| read_optional("CLAUDE_CODE_COORDINATOR_MODE"))
            .unwrap_or(false);
        let workflow_scripts = read_default_enabled("ALLTHECODES_WORKFLOW_SCRIPTS");
        let push_notification_remote_bridge =
            read_default_enabled("ALLTHECODES_PUSH_NOTIFICATION_REMOTE_BRIDGE");
        let goal_tools = read_default_enabled("ALLTHECODES_GOAL_TOOLS");
        let multi_agent_v2 = read_default_enabled("ALLTHECODES_MULTI_AGENT_V2");
        let mut kairos_brief = read("FEATURE_KAIROS_BRIEF");
        let mut kairos_channels = read("FEATURE_KAIROS_CHANNELS");
        let mut kairos_push_notification = read("FEATURE_KAIROS_PUSH_NOTIFICATION");
        let mut kairos_github_webhooks = read("FEATURE_KAIROS_GITHUB_WEBHOOKS");
        let mut proactive = read("FEATURE_PROACTIVE");

        // --- dependency enforcement ---
        // Children require `kairos`.  Disable + warn when parent is missing.
        if !kairos {
            if kairos_brief {
                tracing::warn!(
                    "FEATURE_KAIROS_BRIEF is set but FEATURE_KAIROS is not enabled; \
                     disabling kairos_brief"
                );
                kairos_brief = false;
            }
            if kairos_channels {
                tracing::warn!(
                    "FEATURE_KAIROS_CHANNELS is set but FEATURE_KAIROS is not enabled; \
                     disabling kairos_channels"
                );
                kairos_channels = false;
            }
            if kairos_push_notification {
                tracing::warn!(
                    "FEATURE_KAIROS_PUSH_NOTIFICATION is set but FEATURE_KAIROS is not enabled; \
                     disabling kairos_push_notification"
                );
                kairos_push_notification = false;
            }
            if kairos_github_webhooks {
                tracing::warn!(
                    "FEATURE_KAIROS_GITHUB_WEBHOOKS is set but FEATURE_KAIROS is not enabled; \
                     disabling kairos_github_webhooks"
                );
                kairos_github_webhooks = false;
            }
        }

        // `proactive` is implied when `kairos` is on.
        if kairos {
            proactive = true;
        }

        Self {
            kairos,
            kairos_brief,
            kairos_channels,
            kairos_push_notification,
            kairos_github_webhooks,
            proactive,
            mcp_skills,
            experimental_skill_search,
            remote_url_discovery,
            team_memory,
            subagent_dashboard,
            agent_teams,
            coordinator,
            workflow_scripts,
            push_notification_remote_bridge,
            goal_tools,
            multi_agent_v2,
        }
    }

    /// Enable every known experimental gate for the current process.
    pub fn all_enabled() -> Self {
        Self {
            kairos: true,
            kairos_brief: true,
            kairos_channels: true,
            kairos_push_notification: true,
            kairos_github_webhooks: true,
            proactive: true,
            mcp_skills: true,
            experimental_skill_search: true,
            remote_url_discovery: true,
            team_memory: true,
            subagent_dashboard: true,
            agent_teams: true,
            coordinator: true,
            workflow_scripts: true,
            push_notification_remote_bridge: true,
            goal_tools: true,
            multi_agent_v2: true,
        }
    }

    /// Disable every known experimental gate for the current process.
    pub fn all_disabled() -> Self {
        Self::default()
    }

    /// Query whether a specific [`Feature`] is enabled.
    pub fn is_enabled(&self, feature: Feature) -> bool {
        match feature {
            Feature::Kairos => self.kairos,
            Feature::KairosBrief => self.kairos_brief,
            Feature::KairosChannels => self.kairos_channels,
            Feature::KairosPushNotification => self.kairos_push_notification,
            Feature::KairosGithubWebhooks => self.kairos_github_webhooks,
            Feature::Proactive => self.proactive,
            Feature::McpSkills => self.mcp_skills,
            Feature::ExperimentalSkillSearch => self.experimental_skill_search,
            Feature::RemoteUrlDiscovery => self.remote_url_discovery,
            Feature::TeamMemory => self.team_memory,
            Feature::SubagentDashboard => self.subagent_dashboard,
            Feature::AgentTeams => self.agent_teams,
            Feature::Coordinator => self.coordinator,
            Feature::WorkflowScripts => self.workflow_scripts,
            Feature::PushNotificationRemoteBridge => self.push_notification_remote_bridge,
            Feature::GoalTools => self.goal_tools,
            Feature::MultiAgentV2 => self.multi_agent_v2,
        }
    }
}

/// Resolve the KAIROS-specific desired/effective profile and provenance.
pub fn resolve_kairos_profile(
    desired: &KairosFeatureProfile,
    mut sources: KairosProfileSources,
    iter: impl IntoIterator<Item = (String, String)>,
) -> KairosProfileResolution {
    let env: HashMap<String, String> = iter.into_iter().collect();
    let mut effective = desired.clone();
    let parse = |key: &str| {
        env.get(key).map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        })
    };

    if let Some(value) = parse("FEATURE_KAIROS") {
        effective.enabled = value;
        sources.enabled = KairosValueSource::Environment;
    }
    if let Some(value) = parse("FEATURE_KAIROS_BRIEF") {
        effective.brief = value;
        sources.brief = KairosValueSource::Environment;
    }
    if let Some(value) = parse("FEATURE_KAIROS_CHANNELS") {
        effective.channels = value;
        sources.channels = KairosValueSource::Environment;
    }
    if let Some(value) = parse("FEATURE_KAIROS_PUSH_NOTIFICATION") {
        effective.push_notifications = value;
        sources.push_notifications = KairosValueSource::Environment;
    }
    if let Some(value) = parse("FEATURE_KAIROS_GITHUB_WEBHOOKS") {
        effective.github_webhooks = value;
        sources.github_webhooks = KairosValueSource::Environment;
    }
    if let Some(value) = parse("FEATURE_PROACTIVE") {
        effective.proactive = value;
        sources.proactive = KairosValueSource::Environment;
    }

    let (effective, diagnostics) = effective.normalized();
    KairosProfileResolution {
        desired: desired.clone(),
        effective,
        sources,
        diagnostics,
    }
}

pub fn resolve_loaded_kairos(settings: &LoadedSettings) -> KairosProfileResolution {
    resolve_kairos_profile(
        &settings.effective.kairos,
        settings.kairos_sources(),
        std::env::vars(),
    )
}

// ---------------------------------------------------------------------------
// Global singleton
// ---------------------------------------------------------------------------

/// Global feature flags initialised once from environment variables.
pub static FLAGS: LazyLock<FeatureFlags> = LazyLock::new(FeatureFlags::from_env);
static SETTINGS_BASELINE: LazyLock<RwLock<Option<FeatureFlags>>> =
    LazyLock::new(|| RwLock::new(None));
static RUNTIME_OVERRIDE: LazyLock<RwLock<Option<FeatureFlags>>> =
    LazyLock::new(|| RwLock::new(None));

/// Convenience: query the global singleton for a specific feature.
pub fn enabled(feature: Feature) -> bool {
    current().is_enabled(feature)
}

/// Effective flags after applying any session-local runtime override.
pub fn current() -> FeatureFlags {
    runtime_override()
        .or_else(settings_baseline)
        .unwrap_or_else(|| FLAGS.clone())
}

pub fn settings_baseline() -> Option<FeatureFlags> {
    SETTINGS_BASELINE
        .read()
        .ok()
        .and_then(|guard| guard.clone())
}

pub fn set_settings_baseline(flags: FeatureFlags) {
    if let Ok(mut guard) = SETTINGS_BASELINE.write() {
        *guard = Some(flags);
    }
}

#[cfg(test)]
fn clear_settings_baseline() {
    if let Ok(mut guard) = SETTINGS_BASELINE.write() {
        *guard = None;
    }
}

/// Session-local runtime override used by `/experimental`.
pub fn runtime_override() -> Option<FeatureFlags> {
    RUNTIME_OVERRIDE.read().ok().and_then(|guard| guard.clone())
}

/// Replace the session-local runtime override.
pub fn set_runtime_override(flags: FeatureFlags) {
    if let Ok(mut guard) = RUNTIME_OVERRIDE.write() {
        *guard = Some(flags);
    }
}

/// Return feature gates to their startup environment values.
pub fn clear_runtime_override() {
    if let Ok(mut guard) = RUNTIME_OVERRIDE.write() {
        *guard = None;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build flags from a slice of `(&str, &str)` pairs.
    fn flags(pairs: &[(&str, &str)]) -> FeatureFlags {
        FeatureFlags::from_env_iter(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())))
    }

    #[test]
    fn defaults_match_stable_and_experimental_gate_policy() {
        let f = flags(&[]);
        assert!(!f.kairos);
        assert!(!f.kairos_brief);
        assert!(!f.kairos_channels);
        assert!(!f.kairos_push_notification);
        assert!(!f.kairos_github_webhooks);
        assert!(!f.proactive);
        assert!(!f.subagent_dashboard);
        assert!(!f.agent_teams);
        assert!(!f.coordinator);
        assert!(!f.mcp_skills);
        assert!(!f.experimental_skill_search);
        assert!(!f.remote_url_discovery);
        assert!(
            f.workflow_scripts,
            "full-build workflow tools default enabled"
        );
        assert!(
            f.push_notification_remote_bridge,
            "remote push bridge defaults enabled"
        );
        assert!(f.goal_tools, "goal tools default enabled");
        assert!(f.multi_agent_v2, "multi-agent v2 defaults enabled");
    }

    #[test]
    fn kairos_enables_proactive() {
        let f = flags(&[("FEATURE_KAIROS", "1")]);
        assert!(f.kairos);
        assert!(f.proactive, "proactive should be implied by kairos");
    }

    #[test]
    fn proactive_standalone() {
        let f = flags(&[("FEATURE_PROACTIVE", "true")]);
        assert!(!f.kairos);
        assert!(f.proactive, "proactive should work standalone");
    }

    #[test]
    fn kairos_and_standalone_proactive_contracts_are_locked() {
        let kairos = flags(&[("FEATURE_KAIROS", "1")]);
        assert!(kairos.kairos);
        assert!(kairos.proactive);

        let proactive = flags(&[("FEATURE_PROACTIVE", "1")]);
        assert!(!proactive.kairos);
        assert!(proactive.proactive);
    }

    #[test]
    fn brief_without_kairos_is_disabled() {
        let f = flags(&[("FEATURE_KAIROS_BRIEF", "1")]);
        assert!(
            !f.kairos_brief,
            "kairos_brief must be disabled when kairos is off"
        );
    }

    #[test]
    fn channels_without_kairos_is_disabled() {
        let f = flags(&[("FEATURE_KAIROS_CHANNELS", "true")]);
        assert!(
            !f.kairos_channels,
            "kairos_channels must be disabled when kairos is off"
        );
    }

    #[test]
    fn push_notification_without_kairos_is_disabled() {
        let f = flags(&[("FEATURE_KAIROS_PUSH_NOTIFICATION", "1")]);
        assert!(
            !f.kairos_push_notification,
            "kairos_push_notification must be disabled when kairos is off"
        );
    }

    #[test]
    fn github_webhooks_without_kairos_is_disabled() {
        let f = flags(&[("FEATURE_KAIROS_GITHUB_WEBHOOKS", "1")]);
        assert!(
            !f.kairos_github_webhooks,
            "kairos_github_webhooks must be disabled when kairos is off"
        );
    }

    #[test]
    fn children_enabled_when_kairos_on() {
        let f = flags(&[
            ("FEATURE_KAIROS", "1"),
            ("FEATURE_KAIROS_BRIEF", "true"),
            ("FEATURE_KAIROS_CHANNELS", "1"),
            ("FEATURE_KAIROS_PUSH_NOTIFICATION", "TRUE"),
            ("FEATURE_KAIROS_GITHUB_WEBHOOKS", "1"),
        ]);
        assert!(f.kairos);
        assert!(f.kairos_brief);
        assert!(f.kairos_channels);
        assert!(f.kairos_push_notification);
        assert!(f.kairos_github_webhooks);
        assert!(f.proactive);
    }

    #[test]
    fn is_enabled_query() {
        let f = flags(&[("FEATURE_KAIROS", "1"), ("FEATURE_KAIROS_BRIEF", "1")]);
        assert!(f.is_enabled(Feature::Kairos));
        assert!(f.is_enabled(Feature::KairosBrief));
        assert!(!f.is_enabled(Feature::KairosChannels));
        assert!(!f.is_enabled(Feature::McpSkills));
        assert!(!f.is_enabled(Feature::ExperimentalSkillSearch));
        assert!(!f.is_enabled(Feature::RemoteUrlDiscovery));
        assert!(f.is_enabled(Feature::Proactive));
    }

    #[test]
    fn kairos_does_not_imply_search_feature_gates() {
        let f = flags(&[("FEATURE_KAIROS", "1")]);
        assert!(f.kairos);
        assert!(f.proactive, "kairos should still imply proactive");
        assert!(
            !f.mcp_skills,
            "FEATURE_KAIROS must not imply FEATURE_MCP_SKILLS"
        );
        assert!(
            !f.experimental_skill_search,
            "FEATURE_KAIROS must not imply FEATURE_EXPERIMENTAL_SKILL_SEARCH"
        );
        assert!(
            !f.remote_url_discovery,
            "FEATURE_KAIROS must not imply FEATURE_REMOTE_URL_DISCOVERY"
        );
    }

    #[test]
    fn search_feature_gates_read_env_vars() {
        let f = flags(&[
            ("FEATURE_MCP_SKILLS", "1"),
            ("FEATURE_EXPERIMENTAL_SKILL_SEARCH", "true"),
            ("FEATURE_REMOTE_URL_DISCOVERY", "yes"),
        ]);
        assert!(f.mcp_skills);
        assert!(f.experimental_skill_search);
        assert!(f.remote_url_discovery);
        assert!(f.is_enabled(Feature::McpSkills));
        assert!(f.is_enabled(Feature::ExperimentalSkillSearch));
        assert!(f.is_enabled(Feature::RemoteUrlDiscovery));
    }

    #[test]
    fn agent_teams_reads_new_env_var() {
        let f = flags(&[("ALLTHECODES_EXPERIMENTAL_AGENT_TEAMS", "yes")]);
        assert!(f.agent_teams);
        assert!(f.is_enabled(Feature::AgentTeams));
    }

    #[test]
    fn agent_teams_reads_upstream_compat_env_var() {
        let f = flags(&[("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS", "1")]);
        assert!(f.agent_teams);
        assert!(f.is_enabled(Feature::AgentTeams));
    }

    #[test]
    fn agent_teams_native_disabled_takes_precedence_over_compat_enabled() {
        let f = flags(&[
            ("FEATURE_AGENT_TEAMS", "0"),
            ("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS", "1"),
        ]);
        assert!(!f.agent_teams);

        let f = flags(&[
            ("ALLTHECODES_EXPERIMENTAL_AGENT_TEAMS", "0"),
            ("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS", "1"),
        ]);
        assert!(!f.agent_teams);
    }

    #[test]
    fn coordinator_reads_new_env_var() {
        let f = flags(&[("ALLTHECODES_COORDINATOR_MODE", "true")]);
        assert!(f.coordinator);
        assert!(f.is_enabled(Feature::Coordinator));
    }

    #[test]
    fn coordinator_reads_upstream_compat_env_var() {
        let f = flags(&[("CLAUDE_CODE_COORDINATOR_MODE", "true")]);
        assert!(f.coordinator);
        assert!(f.is_enabled(Feature::Coordinator));
    }

    #[test]
    fn allthecodes_env_takes_precedence_when_compat_env_is_disabled() {
        let f = flags(&[
            ("ALLTHECODES_COORDINATOR_MODE", "1"),
            ("CLAUDE_CODE_COORDINATOR_MODE", "0"),
        ]);
        assert!(f.coordinator);
    }

    #[test]
    fn coordinator_native_disabled_takes_precedence_over_compat_enabled() {
        let f = flags(&[
            ("ALLTHECODES_COORDINATOR_MODE", "0"),
            ("CLAUDE_CODE_COORDINATOR_MODE", "1"),
        ]);
        assert!(!f.coordinator);
    }

    #[test]
    fn all_enabled_covers_feature_descriptors() {
        let f = FeatureFlags::all_enabled();
        for descriptor in feature_descriptors() {
            assert!(
                f.is_enabled(descriptor.feature),
                "{} should be enabled",
                descriptor.label
            );
        }
    }

    #[test]
    fn feature_descriptors_are_unique_and_complete() {
        let descriptors = feature_descriptors();
        assert_eq!(descriptors.len(), 17);

        let mut labels: Vec<_> = descriptors
            .iter()
            .map(|descriptor| descriptor.label)
            .collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), descriptors.len());

        let github = descriptors
            .iter()
            .find(|descriptor| descriptor.feature == Feature::KairosGithubWebhooks)
            .expect("github webhook descriptor is exposed");
        assert_eq!(github.env_var, "FEATURE_KAIROS_GITHUB_WEBHOOKS");
        assert!(github.description.contains("webhook"));

        let mcp_skills = descriptors
            .iter()
            .find(|descriptor| descriptor.feature == Feature::McpSkills)
            .expect("mcp skills descriptor is exposed");
        assert_eq!(mcp_skills.env_var, "FEATURE_MCP_SKILLS");
        assert_eq!(mcp_skills.label, "mcp_skills");

        let experimental_skill_search = descriptors
            .iter()
            .find(|descriptor| descriptor.feature == Feature::ExperimentalSkillSearch)
            .expect("experimental skill search descriptor is exposed");
        assert_eq!(
            experimental_skill_search.env_var,
            "FEATURE_EXPERIMENTAL_SKILL_SEARCH"
        );
        assert_eq!(experimental_skill_search.label, "experimental_skill_search");

        let remote_url_discovery = descriptors
            .iter()
            .find(|descriptor| descriptor.feature == Feature::RemoteUrlDiscovery)
            .expect("remote URL discovery descriptor is exposed");
        assert_eq!(remote_url_discovery.env_var, "FEATURE_REMOTE_URL_DISCOVERY");
        assert_eq!(remote_url_discovery.label, "remote_url_discovery");
        assert!(remote_url_discovery
            .description
            .contains("skill/plugin/MCP"));

        let agent_teams = descriptors
            .iter()
            .find(|descriptor| descriptor.feature == Feature::AgentTeams)
            .expect("agent teams descriptor is exposed");
        assert!(
            agent_teams.description.contains("beta"),
            "Agent Teams is a released beta surface, not a hidden experiment"
        );

        let multi_agent_v2 = descriptors
            .iter()
            .find(|descriptor| descriptor.feature == Feature::MultiAgentV2)
            .expect("multi-agent v2 descriptor is exposed");
        assert!(
            multi_agent_v2
                .description
                .contains("list/followup/wait/close"),
            "MultiAgentV2 should describe the currently gated v2 aliases"
        );
        assert!(
            !multi_agent_v2.description.contains("spawn/send"),
            "TeamSpawn and SendMessage are controlled by AgentTeams, not MultiAgentV2"
        );
    }

    #[test]
    fn false_values_are_not_enabled() {
        let f = flags(&[
            ("FEATURE_KAIROS", "0"),
            ("FEATURE_PROACTIVE", "false"),
            ("ALLTHECODES_GOAL_TOOLS", "no"),
            ("ALLTHECODES_MULTI_AGENT_V2", "0"),
        ]);
        assert!(!f.kairos);
        assert!(!f.proactive);
        assert!(!f.goal_tools);
        assert!(!f.multi_agent_v2);
    }

    #[test]
    fn trimmed_and_case_insensitive() {
        let f = flags(&[
            ("FEATURE_KAIROS", " True "),
            ("FEATURE_KAIROS_BRIEF", " 1 "),
        ]);
        assert!(f.kairos);
        assert!(f.kairos_brief);
    }

    #[test]
    fn team_memory_standalone() {
        let f = flags(&[("FEATURE_TEAMMEM", "1")]);
        assert!(f.team_memory);
        assert!(!f.kairos, "team_memory should not imply kairos");
    }

    #[test]
    fn team_memory_default_off() {
        let f = flags(&[]);
        assert!(!f.team_memory);
    }

    #[test]
    fn subagent_dashboard_standalone() {
        let f = flags(&[("FEATURE_SUBAGENT_DASHBOARD", "1")]);
        assert!(f.subagent_dashboard);
        assert!(!f.kairos);
    }

    #[test]
    fn typed_profile_seeds_flags_and_environment_can_override_it() {
        let profile = KairosFeatureProfile {
            enabled: true,
            brief: true,
            channels: true,
            ..KairosFeatureProfile::default()
        };
        let flags = FeatureFlags::from_profile_and_env_iter(
            &profile,
            [("FEATURE_KAIROS_BRIEF".to_string(), "0".to_string())],
        );

        assert!(flags.kairos);
        assert!(!flags.kairos_brief);
        assert!(flags.kairos_channels);
        assert!(flags.proactive);
    }

    #[test]
    #[serial_test::serial]
    fn runtime_override_wins_over_settings_baseline() {
        clear_settings_baseline();
        clear_runtime_override();
        let mut baseline = FeatureFlags::all_disabled();
        baseline.kairos = true;
        set_settings_baseline(baseline);
        assert!(current().kairos);

        set_runtime_override(FeatureFlags::all_disabled());
        assert!(!current().kairos);

        clear_runtime_override();
        clear_settings_baseline();
    }

    #[test]
    fn kairos_resolution_reports_environment_provenance_and_dependencies() {
        let desired = KairosFeatureProfile {
            enabled: true,
            brief: true,
            ..KairosFeatureProfile::default()
        };
        let resolution = resolve_kairos_profile(
            &desired,
            KairosProfileSources {
                enabled: KairosValueSource::Local,
                brief: KairosValueSource::Local,
                ..KairosProfileSources::default()
            },
            [("FEATURE_KAIROS".to_string(), "false".to_string())],
        );

        assert!(resolution.desired.enabled);
        assert!(!resolution.effective.enabled);
        assert!(!resolution.effective.brief);
        assert_eq!(resolution.sources.enabled, KairosValueSource::Environment);
        assert_eq!(resolution.sources.brief, KairosValueSource::Local);
        assert_eq!(resolution.diagnostics.len(), 1);
    }
}
