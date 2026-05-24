//! Plugin version specification and compatibility checking.
//!
//! Uses the `semver` crate for requirement matching. All version strings
//! follow semver conventions.

use serde::{Deserialize, Serialize};

/// A version requirement for plugin or engine compatibility.
///
/// Wraps a semver requirement string (e.g. `"^1.0.0"`, `">=2.0.0 <3.0.0"`)
/// and provides matching against concrete versions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionRequirement {
    /// Raw requirement string in semver format.
    pub spec: String,
}

impl VersionRequirement {
    /// Create a new version requirement.
    pub fn new(spec: impl Into<String>) -> Self {
        Self { spec: spec.into() }
    }

    /// Check whether a concrete version satisfies this requirement.
    pub fn satisfied_by(&self, version: &str) -> bool {
        // If both version and requirement parse, use semver matching
        if let (Ok(ver), Ok(req)) = (
            semver::Version::parse(version),
            semver::VersionReq::parse(&self.spec),
        ) {
            return req.matches(&ver);
        }
        // Fallback: exact string comparison
        version == self.spec
    }
}

/// Check whether two version strings are compatible in a major-version sense.
///
/// Returns `true` when `required` can be satisfied by `installed`, using
/// semver range matching if `required` parses as a range, or exact/prefix
/// matching otherwise.
pub fn check_version_compatibility(required: &str, installed: &str) -> bool {
    let req = VersionRequirement::new(required);
    req.satisfied_by(installed)
}

/// Engine compatibility check.
///
/// Returns `true` if `plugin_min_app_version` is compatible with the
/// running engine version.
pub fn check_engine_compatibility(
    plugin_min_app_version: Option<&str>,
    engine_version: &str,
) -> bool {
    match plugin_min_app_version {
        Some(min_ver) => {
            let min = VersionRequirement::new(min_ver);
            min.satisfied_by(engine_version)
        }
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_version_match() {
        // = is used for exact matching in semver
        let req = VersionRequirement::new("=1.2.3");
        assert!(req.satisfied_by("1.2.3"));
        assert!(!req.satisfied_by("1.2.4"));
        assert!(!req.satisfied_by("2.0.0"));
    }

    #[test]
    fn test_caret_version() {
        let req = VersionRequirement::new("^1.2.3");
        assert!(req.satisfied_by("1.2.3"));
        assert!(req.satisfied_by("1.5.0"));
        assert!(!req.satisfied_by("2.0.0"));
    }

    #[test]
    fn test_tilde_version() {
        let req = VersionRequirement::new("~1.2.3");
        assert!(req.satisfied_by("1.2.3"));
        assert!(req.satisfied_by("1.2.9"));
        assert!(!req.satisfied_by("1.3.0"));
        assert!(!req.satisfied_by("2.0.0"));
    }

    #[test]
    fn test_minimum_version() {
        let req = VersionRequirement::new(">=1.0.0");
        assert!(req.satisfied_by("1.0.0"));
        assert!(req.satisfied_by("2.5.0"));
        assert!(!req.satisfied_by("0.9.0"));
    }

    #[test]
    fn test_range_version() {
        // Comma-separated range in semver
        let req = VersionRequirement::new(">=1.0.0, <2.0.0");
        assert!(req.satisfied_by("1.5.0"));
        assert!(!req.satisfied_by("2.0.0"));
        assert!(!req.satisfied_by("0.9.0"));
    }

    #[test]
    fn test_compatibility_function() {
        assert!(check_version_compatibility("^1.0.0", "1.2.3"));
        assert!(!check_version_compatibility("^2.0.0", "1.9.9"));
        assert!(check_version_compatibility(">=1.0.0", "3.0.0"));
    }

    #[test]
    fn test_engine_compatibility() {
        assert!(check_engine_compatibility(None, "1.0.0"));
        assert!(check_engine_compatibility(Some("^1.0.0"), "1.5.0"));
        assert!(!check_engine_compatibility(Some("^2.0.0"), "1.9.9"));
    }

    #[test]
    fn test_invalid_version_returns_false() {
        let req = VersionRequirement::new("^1.0.0");
        assert!(!req.satisfied_by("not-a-version"));
    }

    #[test]
    fn test_invalid_requirement_falls_back_to_exact() {
        let req = VersionRequirement::new("not-a-req");
        assert!(req.satisfied_by("not-a-req"));
        assert!(!req.satisfied_by("something-else"));
    }

    #[test]
    fn test_serde_roundtrip() {
        let req = VersionRequirement::new("^1.2.3");
        let json = serde_json::to_string(&req).unwrap();
        let back: VersionRequirement = serde_json::from_str(&json).unwrap();
        assert_eq!(back.spec, "^1.2.3");
    }
}
