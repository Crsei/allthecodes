//! Plugin policy enforcement.
//!
//! Enforces managed policy decisions for plugin installation and usage,
//! consuming Lane A's `ManagedPolicy` types.

use serde::{Deserialize, Serialize};

use crate::PluginSource;

/// Result of a policy decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyDecision {
    /// Plugin is allowed by policy.
    Allowed,
    /// Plugin is blocked by policy.
    Blocked {
        /// Human-readable reason.
        reason: String,
    },
    /// Plugin is flagged for review but not blocked.
    Flagged {
        /// Reason for flagging.
        reason: String,
    },
}

impl PolicyDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, PolicyDecision::Allowed)
    }

    pub fn is_blocked(&self) -> bool {
        matches!(self, PolicyDecision::Blocked { .. })
    }
}

/// Policy enforcer for plugin operations.
///
/// Consumes `allthecodes_config::mdm::settings::ManagedPolicy` to make policy decisions.
pub struct PluginPolicyEnforcer;

impl PluginPolicyEnforcer {
    /// Check whether a plugin is allowed to be installed based on managed policy.
    pub fn check_plugin_allowed(
        _plugin_id: &str,
        source: &PluginSource,
        policy: Option<&allthecodes_config::mdm::ManagedPolicy>,
    ) -> PolicyDecision {
        let Some(policy) = policy else {
            return PolicyDecision::Allowed;
        };

        let Some(install_policy) = &policy.plugin_install_policy else {
            return PolicyDecision::Allowed;
        };

        // Check allowed sources
        if !install_policy.allowed_sources.is_empty() {
            let source_category = categorize_source(source);
            let source_allowed = install_policy.allowed_sources.iter().any(|allowed| {
                let allowed_str = format!("{:?}", allowed).to_lowercase();
                source_category == allowed_str
            });

            if !source_allowed {
                return PolicyDecision::Blocked {
                    reason: format!(
                        "Plugin source '{:?}' is not in the allowed sources list",
                        source
                    ),
                };
            }
        }

        // Check max plugins (deferred to installation flow)

        PolicyDecision::Allowed
    }

    /// Check if signature is required for this plugin installation.
    pub fn is_signature_required(policy: Option<&allthecodes_config::mdm::ManagedPolicy>) -> bool {
        policy
            .and_then(|p| p.plugin_install_policy.as_ref())
            .and_then(|pip| pip.require_signature)
            .unwrap_or(false)
    }

    /// Get the maximum number of allowed plugins.
    pub fn max_plugins(policy: Option<&allthecodes_config::mdm::ManagedPolicy>) -> Option<u32> {
        policy
            .and_then(|p| p.plugin_install_policy.as_ref())
            .and_then(|pip| pip.max_plugins)
    }
}

/// Categorize a PluginSource into a policy source category string.
fn categorize_source(source: &PluginSource) -> String {
    match source {
        PluginSource::Npm { .. } => "marketplace".to_string(),
        PluginSource::GitHub { .. } => "git".to_string(),
        PluginSource::Git { .. } => "git".to_string(),
        PluginSource::Url { .. } => "url".to_string(),
        PluginSource::Marketplace { .. } => "marketplace".to_string(),
        PluginSource::Local { .. } => "local".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_config::mdm::{
        ManagedPolicy, PluginInstallPolicy, PluginSource as PolicyPluginSource,
    };

    fn make_policy_with_sources(sources: Vec<PolicyPluginSource>) -> ManagedPolicy {
        ManagedPolicy {
            plugin_install_policy: Some(PluginInstallPolicy {
                allowed_sources: sources,
                require_signature: None,
                max_plugins: None,
            }),
            ..ManagedPolicy::default()
        }
    }

    #[test]
    fn test_no_policy_allows_all() {
        let decision = PluginPolicyEnforcer::check_plugin_allowed(
            "test",
            &PluginSource::Local {
                path: "/tmp".into(),
            },
            None,
        );
        assert!(decision.is_allowed());
    }

    #[test]
    fn test_allowed_source() {
        let policy = make_policy_with_sources(vec![PolicyPluginSource::Local]);
        let decision = PluginPolicyEnforcer::check_plugin_allowed(
            "test",
            &PluginSource::Local {
                path: "/tmp".into(),
            },
            Some(&policy),
        );
        assert!(decision.is_allowed());
    }

    #[test]
    fn test_blocked_source() {
        let policy = make_policy_with_sources(vec![PolicyPluginSource::Marketplace]);
        let decision = PluginPolicyEnforcer::check_plugin_allowed(
            "test",
            &PluginSource::Local {
                path: "/tmp".into(),
            },
            Some(&policy),
        );
        assert!(decision.is_blocked());
    }

    #[test]
    fn test_empty_allowed_sources_allows_all() {
        let policy = make_policy_with_sources(vec![]);
        let decision = PluginPolicyEnforcer::check_plugin_allowed(
            "test",
            &PluginSource::Npm {
                package: "pkg".into(),
                version: None,
            },
            Some(&policy),
        );
        assert!(decision.is_allowed());
    }

    #[test]
    fn test_marketplace_source_category() {
        let policy = make_policy_with_sources(vec![PolicyPluginSource::Marketplace]);
        let decision = PluginPolicyEnforcer::check_plugin_allowed(
            "test",
            &PluginSource::Marketplace {
                id: "test".into(),
                source_name: "local-market".into(),
            },
            Some(&policy),
        );
        assert!(decision.is_allowed());
    }

    #[test]
    fn test_signature_required() {
        let policy = ManagedPolicy {
            plugin_install_policy: Some(PluginInstallPolicy {
                allowed_sources: vec![],
                require_signature: Some(true),
                max_plugins: None,
            }),
            ..ManagedPolicy::default()
        };

        assert!(PluginPolicyEnforcer::is_signature_required(Some(&policy)));
        assert!(!PluginPolicyEnforcer::is_signature_required(None));
    }

    #[test]
    fn test_max_plugins() {
        let policy = ManagedPolicy {
            plugin_install_policy: Some(PluginInstallPolicy {
                allowed_sources: vec![],
                require_signature: None,
                max_plugins: Some(5),
            }),
            ..ManagedPolicy::default()
        };

        assert_eq!(PluginPolicyEnforcer::max_plugins(Some(&policy)), Some(5));
        assert_eq!(PluginPolicyEnforcer::max_plugins(None), None);
    }
}
