//! Shared KAIROS configuration, lifecycle, and control-plane DTOs.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[cfg(feature = "schema")]
use schemars::JsonSchema;

/// Fully materialized KAIROS feature profile.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(default, rename_all = "snake_case")]
pub struct KairosFeatureProfile {
    pub enabled: bool,
    pub brief: bool,
    pub channels: bool,
    pub push_notifications: bool,
    pub github_webhooks: bool,
    pub proactive: bool,
}

impl KairosFeatureProfile {
    /// Enforce parent/child feature dependencies without mutating the input.
    pub fn normalized(&self) -> (Self, Vec<KairosDiagnostic>) {
        let mut profile = self.clone();
        let mut diagnostics = Vec::new();

        if !profile.enabled {
            for (enabled, feature) in [
                (&mut profile.brief, "brief"),
                (&mut profile.channels, "channels"),
                (&mut profile.push_notifications, "push_notifications"),
                (&mut profile.github_webhooks, "github_webhooks"),
            ] {
                if *enabled {
                    *enabled = false;
                    diagnostics.push(KairosDiagnostic {
                        code: "kairos_parent_disabled".to_string(),
                        feature: Some(feature.to_string()),
                        message: format!("{feature} requires KAIROS to be enabled"),
                    });
                }
            }
        } else {
            profile.proactive = true;
        }

        (profile, diagnostics)
    }

    pub fn differs_for_restart(&self, running: &Self) -> bool {
        self != running
    }
}

/// Partial profile used by settings and API updates.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(default, rename_all = "snake_case")]
pub struct KairosFeatureProfilePatch {
    pub enabled: Option<bool>,
    pub brief: Option<bool>,
    pub channels: Option<bool>,
    pub push_notifications: Option<bool>,
    pub github_webhooks: Option<bool>,
    pub proactive: Option<bool>,
}

impl KairosFeatureProfilePatch {
    pub fn apply_to(&self, profile: &mut KairosFeatureProfile) {
        if let Some(value) = self.enabled {
            profile.enabled = value;
        }
        if let Some(value) = self.brief {
            profile.brief = value;
        }
        if let Some(value) = self.channels {
            profile.channels = value;
        }
        if let Some(value) = self.push_notifications {
            profile.push_notifications = value;
        }
        if let Some(value) = self.github_webhooks {
            profile.github_webhooks = value;
        }
        if let Some(value) = self.proactive {
            profile.proactive = value;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.enabled.is_none()
            && self.brief.is_none()
            && self.channels.is_none()
            && self.push_notifications.is_none()
            && self.github_webhooks.is_none()
            && self.proactive.is_none()
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum KairosConfigScope {
    User,
    Project,
    #[default]
    Local,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum KairosValueSource {
    #[default]
    Default,
    Managed,
    User,
    UserProfile,
    Project,
    Local,
    Environment,
    Cli,
    Runtime,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(default, rename_all = "snake_case")]
pub struct KairosProfileSources {
    pub enabled: KairosValueSource,
    pub brief: KairosValueSource,
    pub channels: KairosValueSource,
    pub push_notifications: KairosValueSource,
    pub github_webhooks: KairosValueSource,
    pub proactive: KairosValueSource,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct KairosDiagnostic {
    pub code: String,
    pub feature: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(default, rename_all = "snake_case")]
pub struct KairosProfileResolution {
    pub desired: KairosFeatureProfile,
    pub effective: KairosFeatureProfile,
    pub sources: KairosProfileSources,
    pub diagnostics: Vec<KairosDiagnostic>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum KairosLifecycleState {
    #[default]
    Stopped,
    Starting,
    Ready,
    Stopping,
    Restarting,
    Stale,
    Failed,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(default, rename_all = "snake_case")]
pub struct KairosWorkerSnapshot {
    pub worker_id: String,
    pub kind: String,
    pub pid: Option<u32>,
    pub status: String,
    pub restart_count: u32,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(default, rename_all = "snake_case")]
pub struct KairosSupervisorSnapshot {
    pub pid: u32,
    pub health_url: String,
    pub started_at: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(default, rename_all = "snake_case")]
pub struct KairosAutomationSnapshot {
    pub status: String,
    pub proactive_active: bool,
    pub query_running: bool,
    pub pending_input: bool,
    pub terminal_focus: bool,
    pub next_tick_at: Option<String>,
    pub sleeping_until: Option<String>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct KairosLifecycleTransition {
    pub operation_id: String,
    pub action: KairosControlAction,
    pub from: KairosLifecycleState,
    pub to: KairosLifecycleState,
    pub timestamp: String,
    pub error_code: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(default, rename_all = "snake_case")]
pub struct KairosRuntimeSnapshot {
    pub desired: KairosFeatureProfile,
    pub effective: KairosFeatureProfile,
    pub running: Option<KairosFeatureProfile>,
    pub sources: KairosProfileSources,
    pub diagnostics: Vec<KairosDiagnostic>,
    pub lifecycle: KairosLifecycleState,
    pub restart_required: bool,
    pub supervisor: Option<KairosSupervisorSnapshot>,
    pub workers: Vec<KairosWorkerSnapshot>,
    pub automation: Option<KairosAutomationSnapshot>,
    pub last_transition: Option<KairosLifecycleTransition>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub extensions: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum KairosControlAction {
    #[default]
    Start,
    Stop,
    Restart,
    Reconcile,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(default, rename_all = "snake_case")]
pub struct KairosControlRequest {
    pub action: KairosControlAction,
    pub cwd: Option<String>,
    pub port: Option<u16>,
    pub readiness_timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(default, rename_all = "snake_case")]
pub struct KairosControlResult {
    pub action: KairosControlAction,
    pub changed: bool,
    pub operation_id: Option<String>,
    pub snapshot: KairosRuntimeSnapshot,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_disables_children_without_parent() {
        let profile = KairosFeatureProfile {
            brief: true,
            channels: true,
            push_notifications: true,
            github_webhooks: true,
            proactive: true,
            ..KairosFeatureProfile::default()
        };

        let (normalized, diagnostics) = profile.normalized();

        assert!(!normalized.brief);
        assert!(!normalized.channels);
        assert!(!normalized.push_notifications);
        assert!(!normalized.github_webhooks);
        assert!(normalized.proactive, "proactive can run standalone");
        assert_eq!(diagnostics.len(), 4);
    }

    #[test]
    fn kairos_implies_proactive() {
        let (normalized, diagnostics) = KairosFeatureProfile {
            enabled: true,
            ..KairosFeatureProfile::default()
        }
        .normalized();

        assert!(normalized.proactive);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn explicit_false_patch_is_applied() {
        let mut profile = KairosFeatureProfile {
            enabled: true,
            brief: true,
            ..KairosFeatureProfile::default()
        };

        KairosFeatureProfilePatch {
            brief: Some(false),
            ..KairosFeatureProfilePatch::default()
        }
        .apply_to(&mut profile);

        assert!(!profile.brief);
        assert!(profile.enabled);
    }
}
