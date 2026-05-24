//! Plugin dependency resolution.
//!
//! Resolves and validates inter-plugin dependencies declared in manifest
//! `dependencies` fields. Supports cycle detection and version compatibility
//! checking.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::manifest::PluginManifest;
use crate::versioning::VersionRequirement;

/// Error type for dependency resolution failures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DependencyError {
    /// A required dependency is missing.
    Missing {
        plugin_id: String,
        requirement: String,
    },
    /// A dependency's version doesn't satisfy the requirement.
    VersionMismatch {
        plugin_id: String,
        required: String,
        installed: String,
    },
    /// A dependency cycle was detected.
    Cycle {
        plugin_id: String,
        chain: Vec<String>,
    },
    /// Unknown dependency error.
    Other(String),
}

impl std::fmt::Display for DependencyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DependencyError::Missing {
                plugin_id,
                requirement,
            } => {
                write!(
                    f,
                    "Missing dependency '{}' (required: {})",
                    plugin_id, requirement
                )
            }
            DependencyError::VersionMismatch {
                plugin_id,
                required,
                installed,
            } => {
                write!(
                    f,
                    "Version mismatch for '{}': required {}, installed {}",
                    plugin_id, required, installed
                )
            }
            DependencyError::Cycle { plugin_id, chain } => {
                write!(
                    f,
                    "Dependency cycle detected involving '{}': {}",
                    plugin_id,
                    chain.join(" -> ")
                )
            }
            DependencyError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

/// A resolved dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedDependency {
    /// Plugin ID of the dependency.
    pub plugin_id: String,
    /// Resolved version.
    pub version: String,
    /// Whether this was already installed.
    pub already_installed: bool,
}

/// Dependency resolver for plugins.
pub struct DependencyResolver;

impl DependencyResolver {
    /// Resolve dependencies for a plugin manifest against available plugins.
    ///
    /// `available_plugins` is a map of plugin_id -> version for all installed
    /// plugins.
    pub fn resolve_dependencies(
        manifest: &PluginManifest,
        available_plugins: &HashMap<String, String>,
    ) -> Result<Vec<ResolvedDependency>, DependencyError> {
        let mut resolved = Vec::new();

        for (dep_id, version_req) in &manifest.dependencies {
            match available_plugins.get(dep_id) {
                Some(installed_version) => {
                    let req = VersionRequirement::new(version_req);
                    if !req.satisfied_by(installed_version) {
                        return Err(DependencyError::VersionMismatch {
                            plugin_id: dep_id.clone(),
                            required: version_req.clone(),
                            installed: installed_version.clone(),
                        });
                    }
                    resolved.push(ResolvedDependency {
                        plugin_id: dep_id.clone(),
                        version: installed_version.clone(),
                        already_installed: true,
                    });
                }
                None => {
                    return Err(DependencyError::Missing {
                        plugin_id: dep_id.clone(),
                        requirement: version_req.clone(),
                    });
                }
            }
        }

        Ok(resolved)
    }

    /// Detect dependency cycles starting from the given manifest.
    ///
    /// Uses DFS to find cycles through the plugin dependency graph.
    pub fn check_dependency_cycle(
        manifest: &PluginManifest,
        all_manifests: &HashMap<String, PluginManifest>,
    ) -> Result<(), DependencyError> {
        let mut visited = HashSet::new();
        let mut in_stack = HashSet::new();
        let mut path = Vec::new();

        Self::dfs(
            &manifest.name,
            all_manifests,
            &mut visited,
            &mut in_stack,
            &mut path,
        )
    }

    fn dfs(
        current: &str,
        all_manifests: &HashMap<String, PluginManifest>,
        visited: &mut HashSet<String>,
        in_stack: &mut HashSet<String>,
        path: &mut Vec<String>,
    ) -> Result<(), DependencyError> {
        if in_stack.contains(current) {
            let chain: Vec<String> = path.iter().skip_while(|p| *p != current).cloned().collect();
            return Err(DependencyError::Cycle {
                plugin_id: current.to_string(),
                chain,
            });
        }

        if visited.contains(current) {
            return Ok(());
        }

        visited.insert(current.to_string());
        in_stack.insert(current.to_string());
        path.push(current.to_string());

        if let Some(manifest) = all_manifests.get(current) {
            for dep_id in manifest.dependencies.keys() {
                Self::dfs(dep_id, all_manifests, visited, in_stack, path)?;
            }
        }

        path.pop();
        in_stack.remove(current);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PluginManifest;
    use std::collections::HashMap;

    fn make_manifest(name: &str, deps: Vec<(&str, &str)>) -> PluginManifest {
        PluginManifest {
            name: name.into(),
            display_name: None,
            version: "1.0.0".into(),
            description: "".into(),
            author: None,
            license: None,
            min_app_version: None,
            tools: vec![],
            skills: vec![],
            mcp_servers: vec![],
            lsp_servers: None,
            commands: vec![],
            dependencies: deps
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            configuration: None,
            agents: None,
            hooks: None,
            output_styles: None,
        }
    }

    #[test]
    fn test_no_dependencies() {
        let manifest = make_manifest("test", vec![]);
        let available = HashMap::new();
        let result = DependencyResolver::resolve_dependencies(&manifest, &available).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_dependency_satisfied() {
        let manifest = make_manifest("test", vec![("dep-a", "^1.0.0")]);
        let mut available = HashMap::new();
        available.insert("dep-a".to_string(), "1.2.3".to_string());

        let result = DependencyResolver::resolve_dependencies(&manifest, &available).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].plugin_id, "dep-a");
        assert!(result[0].already_installed);
    }

    #[test]
    fn test_dependency_missing() {
        let manifest = make_manifest("test", vec![("dep-a", "^1.0.0")]);
        let available = HashMap::new();

        let err = DependencyResolver::resolve_dependencies(&manifest, &available).unwrap_err();
        match err {
            DependencyError::Missing { plugin_id, .. } => {
                assert_eq!(plugin_id, "dep-a");
            }
            _ => panic!("Expected Missing error"),
        }
    }

    #[test]
    fn test_version_mismatch() {
        let manifest = make_manifest("test", vec![("dep-a", "^2.0.0")]);
        let mut available = HashMap::new();
        available.insert("dep-a".to_string(), "1.0.0".to_string());

        let err = DependencyResolver::resolve_dependencies(&manifest, &available).unwrap_err();
        match err {
            DependencyError::VersionMismatch { plugin_id, .. } => {
                assert_eq!(plugin_id, "dep-a");
            }
            _ => panic!("Expected VersionMismatch error"),
        }
    }

    #[test]
    fn test_cycle_detection_self_reference() {
        let mut all = HashMap::new();
        all.insert("a".to_string(), make_manifest("a", vec![("a", "^1.0.0")]));

        let result = DependencyResolver::check_dependency_cycle(&all["a"], &all);
        assert!(result.is_err());
        match result.unwrap_err() {
            DependencyError::Cycle { plugin_id, .. } => {
                assert_eq!(plugin_id, "a");
            }
            _ => panic!("Expected Cycle error"),
        }
    }

    #[test]
    fn test_cycle_detection_a_to_b_to_a() {
        let mut all = HashMap::new();
        all.insert("a".to_string(), make_manifest("a", vec![("b", "^1.0.0")]));
        all.insert("b".to_string(), make_manifest("b", vec![("a", "^1.0.0")]));

        let result = DependencyResolver::check_dependency_cycle(&all["a"], &all);
        assert!(result.is_err());
    }

    #[test]
    fn test_no_cycle_for_linear_deps() {
        let mut all = HashMap::new();
        all.insert("a".to_string(), make_manifest("a", vec![("b", "^1.0.0")]));
        all.insert("b".to_string(), make_manifest("b", vec![("c", "^1.0.0")]));
        all.insert("c".to_string(), make_manifest("c", vec![]));

        let result = DependencyResolver::check_dependency_cycle(&all["a"], &all);
        assert!(result.is_ok());
    }
}
