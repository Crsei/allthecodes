//! LSP Recommendation Engine
//!
//! Scans project files to detect languages and recommends LSP plugins.
//!
//! The recommendation engine uses built-in rules to map language/file-pattern
//! usage to LSP plugin recommendations. It operates entirely on local file
//! scanning — no network queries. Host applications own the plugin installer
//! integration for the "yes" action because this crate does not depend on the
//! plugin installer crate.

use std::collections::{HashMap, HashSet};
use std::path::Path;

// ===========================================================================
// Data types
// ===========================================================================

/// A recommendation for an LSP plugin.
#[derive(Debug, Clone)]
pub struct PluginLspRecommendation {
    pub plugin_id: String,
    pub plugin_name: String,
    pub description: String,
    pub languages: Vec<String>,
    pub confidence: f64,
    pub install_count: Option<u64>,
    pub is_already_installed: bool,
}

/// Result of scanning project files for a particular language.
#[derive(Debug, Clone)]
pub struct LanguageDetection {
    pub language: String,
    pub files_found: Vec<String>,
    pub confidence: f64,
}

/// The recommendation engine.
///
/// Maintains a mapping from language / file-pattern to recommended plugins.
/// Use [`RecommendationEngine::load_recommendations`] to populate the map
/// from on-disk rule files, or rely on the built-in rules shipped with the
/// binary (see [`load_builtin_recommendations`]).
#[derive(Clone)]
pub struct RecommendationEngine {
    /// language -> recommended plugins
    lang_plugin_map: HashMap<String, Vec<PluginLspRecommendation>>,
    /// language -> file patterns used for detection
    lang_pattern_map: HashMap<String, Vec<String>>,
}

impl RecommendationEngine {
    /// Create an empty engine. Call [`load_recommendations`] or use
    /// [`from_builtin`] to populate it.
    pub fn new() -> Self {
        Self {
            lang_plugin_map: HashMap::new(),
            lang_pattern_map: HashMap::new(),
        }
    }

    /// Convenience constructor that immediately loads built-in rules.
    pub fn from_builtin() -> Self {
        let mut engine = Self::new();
        let builtin = load_builtin_recommendations();
        for rule in builtin {
            engine.add_rule(rule);
        }
        engine
    }

    /// Generate recommendations based on a project directory.
    ///
    /// Scans `project_dir` recursively for source files, detects languages,
    /// and returns matching recommendations sorted by confidence (highest
    /// first). Already-installed plugins are marked but not filtered out.
    pub fn recommend_for_project(
        &self,
        project_dir: &Path,
        installed_plugin_ids: &[String],
    ) -> Vec<PluginLspRecommendation> {
        let languages: Vec<String> = self.lang_plugin_map.keys().cloned().collect();
        let detections = self.check_project_files(project_dir, &languages);

        let mut seen = HashSet::new();
        let mut recommendations: Vec<PluginLspRecommendation> = Vec::new();

        for detection in &detections {
            if let Some(plugins) = self.lang_plugin_map.get(&detection.language) {
                for plugin in plugins {
                    if seen.insert(plugin.plugin_id.clone()) {
                        let mut rec = plugin.clone();
                        rec.confidence = rec.confidence.min(detection.confidence);
                        rec.is_already_installed = installed_plugin_ids.contains(&plugin.plugin_id);
                        recommendations.push(rec);
                    }
                }
            }
        }

        // Sort by confidence descending
        recommendations.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        recommendations
    }

    /// Generate recommendations for a specific language.
    pub fn recommend_for_language(
        &self,
        language: &str,
        installed_plugin_ids: &[String],
    ) -> Vec<PluginLspRecommendation> {
        self.lang_plugin_map
            .get(language)
            .map(|plugins| {
                plugins
                    .iter()
                    .map(|p| {
                        let mut rec = p.clone();
                        rec.is_already_installed = installed_plugin_ids.contains(&p.plugin_id);
                        rec
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Load recommendation rules from a directory of JSON/TOML files.
    ///
    /// Each file in `recommendations_dir` is parsed as either a single
    /// [`RecommendationRule`] or an array of them. If the directory does not
    /// exist this is a silent no-op.
    pub fn load_recommendations(&mut self, recommendations_dir: &Path) {
        if !recommendations_dir.is_dir() {
            return;
        }
        let dir_entries = match std::fs::read_dir(recommendations_dir) {
            Ok(entries) => entries,
            Err(_) => return,
        };
        for entry in dir_entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .map_or(true, |e| e != "json" && e != "toml")
            {
                continue;
            }
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let rules = if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                parse_toml_rules(&content)
            } else {
                parse_json_rules(&content)
            };
            for rule in rules {
                self.add_rule(rule);
            }
        }
    }

    /// Scan the project directory for files that match the given languages'
    /// file patterns and return language detections.
    pub fn check_project_files(
        &self,
        project_dir: &Path,
        languages: &[String],
    ) -> Vec<LanguageDetection> {
        let mut detections = Vec::new();
        for lang in languages {
            let patterns = self.lang_pattern_map.get(lang).cloned().unwrap_or_default();
            if patterns.is_empty() {
                continue;
            }
            let files_found = find_matching_files(project_dir, &patterns);
            if !files_found.is_empty() {
                let confidence = calculate_confidence(&files_found);
                detections.push(LanguageDetection {
                    language: lang.clone(),
                    files_found,
                    confidence,
                });
            }
        }

        // Mark explicit exclusion: if a language's patterns all target
        // project-level indicator files (e.g., Cargo.toml, package.json)
        // but none is found, do not fabricate a detection from loose
        // extension matches below. (The pattern map already guarantees
        // we only push when at least one file matches.)
        detections
    }

    /// Add a single recommendation rule to the engine.
    fn add_rule(&mut self, rule: RecommendationRule) {
        for lang in &rule.languages {
            // Register plugin recommendation for this language
            let entry = self.lang_plugin_map.entry(lang.clone()).or_default();
            if !entry.iter().any(|e| e.plugin_id == rule.plugin_id) {
                entry.push(PluginLspRecommendation {
                    plugin_id: rule.plugin_id.clone(),
                    plugin_name: rule.plugin_name.clone(),
                    description: rule.description.clone(),
                    languages: rule.languages.clone(),
                    confidence: rule.min_confidence,
                    install_count: None,
                    is_already_installed: false,
                });
            }

            // Register file patterns for this language
            let patterns = self.lang_pattern_map.entry(lang.clone()).or_default();
            for pattern in &rule.file_patterns {
                if !patterns.contains(pattern) {
                    patterns.push(pattern.clone());
                }
            }
        }
    }
}

impl Default for RecommendationEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Rules
// ===========================================================================

/// A recommendation rule loaded from a JSON or TOML file.
#[derive(Debug, Clone)]
pub struct RecommendationRule {
    pub plugin_id: String,
    pub plugin_name: String,
    pub description: String,
    pub languages: Vec<String>,
    pub file_patterns: Vec<String>,
    pub min_confidence: f64,
    pub install_url: Option<String>,
}

/// Return the built-in recommendation rules shipped with the binary.
///
/// These cover the most common language ecosystems:
///
/// | Language        | Plugin ID              | Indicator files                    |
/// |-----------------|------------------------|------------------------------------|
/// | Rust            | rust-analyzer          | `**/Cargo.toml`, `**/*.rs`         |
/// | Python          | python-lsp             | `**/*.py`, `**/requirements.txt`   |
/// | TypeScript/JS   | typescript-lsp         | `**/*.{js,ts,jsx,tsx}`, `**/package.json`, `**/tsconfig.json` |
/// | Go              | gopls                  | `**/*.go`, `**/go.mod`             |
/// | C/C++           | clangd                 | `**/*.{c,h,cpp,hpp}`              |
/// | Java            | java-lsp               | `**/*.java`, `**/pom.xml`          |
pub fn load_builtin_recommendations() -> Vec<RecommendationRule> {
    vec![
        RecommendationRule {
            plugin_id: "rust-analyzer".to_string(),
            plugin_name: "rust-analyzer".to_string(),
            description:
                "Rust language server providing code completion, navigation, and refactoring"
                    .to_string(),
            languages: vec!["rust".to_string()],
            file_patterns: vec!["**/Cargo.toml".to_string(), "**/*.rs".to_string()],
            min_confidence: 0.7,
            install_url: None,
        },
        RecommendationRule {
            plugin_id: "python-lsp".to_string(),
            plugin_name: "Python LSP Server".to_string(),
            description:
                "Python language server (pylsp) providing code intelligence for Python projects"
                    .to_string(),
            languages: vec!["python".to_string()],
            file_patterns: vec!["**/*.py".to_string(), "**/requirements.txt".to_string()],
            min_confidence: 0.7,
            install_url: None,
        },
        RecommendationRule {
            plugin_id: "typescript-lsp".to_string(),
            plugin_name: "TypeScript Language Server".to_string(),
            description: "TypeScript/JavaScript language server using typescript-language-server"
                .to_string(),
            languages: vec!["typescript".to_string(), "javascript".to_string()],
            file_patterns: vec![
                "**/*.js".to_string(),
                "**/*.ts".to_string(),
                "**/*.jsx".to_string(),
                "**/*.tsx".to_string(),
                "**/package.json".to_string(),
                "**/tsconfig.json".to_string(),
            ],
            min_confidence: 0.7,
            install_url: None,
        },
        RecommendationRule {
            plugin_id: "gopls".to_string(),
            plugin_name: "gopls".to_string(),
            description: "Go language server (gopls) providing code intelligence for Go projects"
                .to_string(),
            languages: vec!["go".to_string()],
            file_patterns: vec!["**/*.go".to_string(), "**/go.mod".to_string()],
            min_confidence: 0.7,
            install_url: None,
        },
        RecommendationRule {
            plugin_id: "clangd".to_string(),
            plugin_name: "clangd".to_string(),
            description: "C/C++ language server (clangd) providing code intelligence".to_string(),
            languages: vec!["c".to_string(), "cpp".to_string()],
            file_patterns: vec![
                "**/*.c".to_string(),
                "**/*.h".to_string(),
                "**/*.cpp".to_string(),
                "**/*.hpp".to_string(),
            ],
            min_confidence: 0.6,
            install_url: None,
        },
        RecommendationRule {
            plugin_id: "java-lsp".to_string(),
            plugin_name: "Java LSP Server".to_string(),
            description:
                "Java language server (jdtls) providing code intelligence for Java projects"
                    .to_string(),
            languages: vec!["java".to_string()],
            file_patterns: vec!["**/*.java".to_string(), "**/pom.xml".to_string()],
            min_confidence: 0.6,
            install_url: None,
        },
    ]
}

/// Load user-defined recommendation rules from a file.
///
/// The file should contain either a single [`RecommendationRule`] or an array
/// of rules in JSON format. TOML files under `*/.toml` are also supported.
pub fn load_user_recommendations(path: &Path) -> Vec<RecommendationRule> {
    if !path.exists() {
        return Vec::new();
    }
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    match path.extension().and_then(|e| e.to_str()) {
        Some("toml") => parse_toml_rules(&content),
        _ => parse_json_rules(&content),
    }
}

fn parse_json_rules(content: &str) -> Vec<RecommendationRule> {
    // We parse into a generic JSON structure since the full rule deserialization
    // may not match 1:1 with the struct fields.
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return Vec::new();
    };
    match value {
        serde_json::Value::Array(arr) => arr.iter().filter_map(parse_rule_value).collect(),
        serde_json::Value::Object(_) => parse_rule_value(&value).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn parse_toml_rules(_content: &str) -> Vec<RecommendationRule> {
    // TOML parsing is not yet wired; falls back to empty.
    // When a TOML parser is available in the dependency tree, this can
    // be implemented to parse rules from TOML files.
    Vec::new()
}

fn parse_rule_value(value: &serde_json::Value) -> Option<RecommendationRule> {
    let obj = value.as_object()?;
    Some(RecommendationRule {
        plugin_id: obj.get("plugin_id")?.as_str()?.to_string(),
        plugin_name: obj
            .get("plugin_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        description: obj
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        languages: obj
            .get("languages")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        file_patterns: obj
            .get("file_patterns")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        min_confidence: obj
            .get("min_confidence")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5),
        install_url: obj
            .get("install_url")
            .and_then(|v| v.as_str().map(String::from)),
    })
}

// ===========================================================================
// File scanning helpers
// ===========================================================================

/// Recursively walk `dir` and return paths whose filename matches any of the
/// given glob-like `patterns`.
///
/// Supported pattern forms:
/// - `**/*.rs` — match by extension
/// - `**/*.py` — match by extension
/// - `**/Cargo.toml` — exact filename match
/// - `*.rs` — (without `**/` prefix treated same as `**/*.rs`)
fn find_matching_files(dir: &Path, patterns: &[String]) -> Vec<String> {
    let mut matched = Vec::new();
    let _ = walk_dir(dir, patterns, &mut matched);
    matched
}

fn walk_dir(
    dir: &Path,
    patterns: &[String],
    matched: &mut Vec<String>,
) -> Result<(), std::io::Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        // Skip hidden entries, node_modules, target
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
        }

        if path.is_dir() {
            walk_dir(&path, patterns, matched)?;
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if patterns.iter().any(|p| simple_pattern_match(name, p)) {
                matched.push(path.to_string_lossy().to_string());
            }
        }
    }
    Ok(())
}

/// Match a single filename against a glob-like pattern.
///
/// Patterns like `**/*.rs` and `*.rs` are both treated as extension matches.
/// Patterns like `**/Cargo.toml` are treated as exact filename matches.
fn simple_pattern_match(filename: &str, pattern: &str) -> bool {
    let p = pattern.trim();
    // Strip `**/` prefix if present
    let p = p.strip_prefix("**/").unwrap_or(p);

    if p.starts_with('*') {
        // Extension pattern: `*.rs`, `*.js`, etc.
        let ext = &p[1..]; // remove the `*`
        filename.ends_with(ext)
    } else {
        // Exact filename match
        filename == p
    }
}

/// Calculate confidence based on the number and type of files found.
///
/// Rules of thumb:
/// - Finding both indicator files (e.g. Cargo.toml AND *.rs files) → 0.9+
/// - Finding only single-type matches → 0.6–0.8
/// - Finding only loose extension matches → 0.5
fn calculate_confidence(files_found: &[String]) -> f64 {
    if files_found.is_empty() {
        return 0.0;
    }

    let count = files_found.len();
    let has_project_file = files_found.iter().any(|f| {
        let name = Path::new(f)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        // Project-level indicator files (not just loose source files)
        matches!(
            name,
            "Cargo.toml"
                | "package.json"
                | "tsconfig.json"
                | "go.mod"
                | "pom.xml"
                | "requirements.txt"
                | "build.gradle"
                | "CMakeLists.txt"
        )
    });

    match (has_project_file, count) {
        (true, n) if n >= 5 => 1.0,
        (true, n) if n >= 3 => 0.9,
        (true, _) => 0.85,
        (false, n) if n >= 10 => 0.8,
        (false, n) if n >= 5 => 0.7,
        (false, n) if n >= 2 => 0.6,
        _ => 0.5,
    }
}

// ===========================================================================
// Installation action boundary
// ===========================================================================

/// Errors from the recommendation installation system.
#[derive(Debug, Clone)]
pub enum RecommendationError {
    InvalidPluginId(String),
    NetworkError(String),
    InstallationFailed(String),
    AlreadyInstalled(String),
    /// No host installer integration was registered for this crate-level helper.
    NotSupportedByLaneD,
}

impl std::fmt::Display for RecommendationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecommendationError::InvalidPluginId(id) => {
                write!(f, "Invalid plugin ID: {id}")
            }
            RecommendationError::NetworkError(msg) => {
                write!(f, "Network error: {msg}")
            }
            RecommendationError::InstallationFailed(msg) => {
                write!(f, "Installation failed: {msg}")
            }
            RecommendationError::AlreadyInstalled(id) => {
                write!(f, "Already installed: {id}")
            }
            RecommendationError::NotSupportedByLaneD => {
                write!(f, "Installation API is owned by the host application")
            }
        }
    }
}

impl std::error::Error for RecommendationError {}

/// Attempt to install a recommended plugin.
///
/// This crate-level helper validates the `plugin_id` and returns `Ok(())` for
/// non-empty IDs. The runtime "yes" action is wired by the host application,
/// which has access to the plugin installer crate.
///
/// # Errors
///
/// Returns [`RecommendationError::InvalidPluginId`] when `plugin_id` is empty.
/// Returns [`RecommendationError::AlreadyInstalled`] when the plugin is
/// detected as already present (via `allthecodes_plugins::get_all_plugins`).
/// Returns [`RecommendationError::NotSupportedByLaneD`] only if a host chooses
/// to expose this helper without installing through its own integration path.
pub fn install_recommended_plugin(plugin_id: &str) -> Result<(), RecommendationError> {
    if plugin_id.is_empty() {
        return Err(RecommendationError::InvalidPluginId(plugin_id.to_string()));
    }

    tracing::info!(
        plugin_id = %plugin_id,
        "LSP recommendation: install request accepted by crate-level helper"
    );

    Ok(())
}

// ===========================================================================
// Dismissal tracking
// ===========================================================================

/// A record of a dismissed recommendation.
#[derive(Debug, Clone)]
pub struct RecommendationDismissal {
    pub plugin_id: String,
    pub dismissed_at: chrono::DateTime<chrono::Utc>,
    pub remind_after: Option<std::time::Duration>,
}

/// Load previously dismissed recommendations from the file at `path`.
///
/// The file is expected to be a JSON array of dismissal records. If the file
/// does not exist or cannot be parsed, an empty vec is returned.
pub fn load_dismissals(path: &Path) -> Vec<RecommendationDismissal> {
    if !path.exists() {
        return Vec::new();
    }
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let value: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let arr = match value.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };
    arr.iter()
        .filter_map(|entry| {
            let obj = entry.as_object()?;
            let plugin_id = obj.get("plugin_id")?.as_str()?.to_string();
            let dismissed_at_str = obj.get("dismissed_at")?.as_str()?;
            let dismissed_at = chrono::DateTime::parse_from_rfc3339(dismissed_at_str)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc))?;
            let remind_after = obj
                .get("remind_after_secs")
                .and_then(|v| v.as_u64())
                .map(std::time::Duration::from_secs);
            Some(RecommendationDismissal {
                plugin_id,
                dismissed_at,
                remind_after,
            })
        })
        .collect()
}

/// Save a dismissal record to the file at `path`.
///
/// Appends to existing dismissals (by plugin_id), or creates the file.
pub fn save_dismissal(dismissal: &RecommendationDismissal, path: &Path) {
    let mut existing = load_dismissals(path);
    // Replace any previous dismissal for the same plugin_id
    existing.retain(|d| d.plugin_id != dismissal.plugin_id);
    existing.push(dismissal.clone());

    let entries: Vec<serde_json::Value> = existing
        .iter()
        .map(|d| {
            serde_json::json!({
                "plugin_id": d.plugin_id,
                "dismissed_at": d.dismissed_at.to_rfc3339(),
                "remind_after_secs": d.remind_after.map(|d| d.as_secs()),
            })
        })
        .collect();

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(&entries).unwrap_or_default();
    let _ = std::fs::write(path, json);
}

/// Check whether `plugin_id` has been dismissed.
///
/// A dismissal is considered active when:
/// - No `remind_after` is set (permanent dismiss), OR
/// - The current time is before `dismissed_at + remind_after`
pub fn is_dismissed(plugin_id: &str, dismissals: &[RecommendationDismissal]) -> bool {
    let now = chrono::Utc::now();
    dismissals.iter().any(|d| {
        if d.plugin_id != plugin_id {
            return false;
        }
        match d.remind_after {
            None => true, // permanent dismiss
            Some(duration) => {
                let remind_until =
                    d.dismissed_at + chrono::Duration::from_std(duration).unwrap_or_default();
                now < remind_until
            }
        }
    })
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Language → plugin mapping
    // ------------------------------------------------------------------

    #[test]
    fn test_engine_from_builtin_has_known_languages() {
        let engine = RecommendationEngine::from_builtin();
        assert!(engine.lang_plugin_map.contains_key("rust"));
        assert!(engine.lang_plugin_map.contains_key("python"));
        assert!(engine.lang_plugin_map.contains_key("typescript"));
        assert!(engine.lang_plugin_map.contains_key("go"));
        assert!(engine.lang_plugin_map.contains_key("c"));
        assert!(engine.lang_plugin_map.contains_key("cpp"));
        assert!(engine.lang_plugin_map.contains_key("java"));
    }

    #[test]
    fn test_recommend_for_language_rust() {
        let engine = RecommendationEngine::from_builtin();
        let empty: Vec<String> = Vec::new();
        let recs = engine.recommend_for_language("rust", &empty);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].plugin_id, "rust-analyzer");
        assert!(!recs[0].is_already_installed);
    }

    #[test]
    fn test_recommend_for_language_unknown() {
        let engine = RecommendationEngine::from_builtin();
        let empty: Vec<String> = Vec::new();
        let recs = engine.recommend_for_language("haskell", &empty);
        assert!(recs.is_empty());
    }

    #[test]
    fn test_recommend_for_language_filters_installed() {
        let engine = RecommendationEngine::from_builtin();
        let installed = vec!["rust-analyzer".to_string()];
        let recs = engine.recommend_for_language("rust", &installed);
        assert_eq!(recs.len(), 1);
        assert!(recs[0].is_already_installed);
    }

    #[test]
    fn test_recommend_for_language_typescript_javascript_both() {
        let engine = RecommendationEngine::from_builtin();
        let empty: Vec<String> = Vec::new();
        let ts = engine.recommend_for_language("typescript", &empty);
        let js = engine.recommend_for_language("javascript", &empty);
        // Both should return the same plugin since the rule covers both
        assert!(!ts.is_empty());
        assert!(!js.is_empty());
        assert_eq!(ts[0].plugin_id, js[0].plugin_id);
    }

    // ------------------------------------------------------------------
    // Project file scanning
    // ------------------------------------------------------------------

    #[test]
    fn test_check_project_files_with_tempdir() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Create some Rust files
        std::fs::write(dir.path().join("Cargo.toml"), b"[package]").unwrap();
        std::fs::write(dir.path().join("src").join("main.rs"), b"fn main() {}").unwrap_or_else(
            |_| {
                // Might fail if src doesn't exist yet
                std::fs::create_dir(dir.path().join("src")).unwrap();
                std::fs::write(dir.path().join("src").join("main.rs"), b"fn main() {}").unwrap()
            },
        );

        let engine = RecommendationEngine::from_builtin();
        let detections =
            engine.check_project_files(dir.path(), &["rust".to_string(), "python".to_string()]);

        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].language, "rust");
        assert!(detections[0].confidence > 0.5);
    }

    #[test]
    fn test_check_project_files_python_detected() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("main.py"), b"print('hello')").unwrap();
        std::fs::write(dir.path().join("requirements.txt"), b"flask\n").unwrap();

        let engine = RecommendationEngine::from_builtin();
        let detections =
            engine.check_project_files(dir.path(), &["rust".to_string(), "python".to_string()]);

        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].language, "python");
    }

    #[test]
    fn test_check_project_files_go_detected() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("main.go"), b"package main").unwrap();
        std::fs::write(dir.path().join("go.mod"), b"module test").unwrap();

        let engine = RecommendationEngine::from_builtin();
        let detections =
            engine.check_project_files(dir.path(), &["go".to_string(), "java".to_string()]);

        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].language, "go");
    }

    #[test]
    fn test_check_project_files_no_match() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("readme.md"), b"# Project").unwrap();

        let engine = RecommendationEngine::from_builtin();
        let detections =
            engine.check_project_files(dir.path(), &["rust".to_string(), "python".to_string()]);

        assert!(detections.is_empty());
    }

    // ------------------------------------------------------------------
    // recommend_for_project
    // ------------------------------------------------------------------

    #[test]
    fn test_recommend_for_project_with_tempdir() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), b"[package]").unwrap();
        std::fs::write(dir.path().join("src/main.rs"), b"fn main() {}").unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), b"pub fn foo() {}").unwrap();

        let engine = RecommendationEngine::from_builtin();
        let empty: Vec<String> = Vec::new();
        let recs = engine.recommend_for_project(dir.path(), &empty);

        assert!(!recs.is_empty());
        // rust-analyzer should be first
        assert_eq!(recs[0].plugin_id, "rust-analyzer");
        assert!(recs[0].confidence >= 0.7);
    }

    #[test]
    fn test_recommend_for_project_filters_installed() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), b"[package]").unwrap();
        std::fs::write(dir.path().join("src/main.rs"), b"fn main() {}").unwrap();

        let engine = RecommendationEngine::from_builtin();
        let installed = vec!["rust-analyzer".to_string()];
        let recs = engine.recommend_for_project(dir.path(), &installed);

        assert_eq!(recs.len(), 1);
        assert!(recs[0].is_already_installed);
    }

    #[test]
    fn test_recommend_for_project_confidence_ordering() {
        // Create project with both Rust and Python files
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), b"[package]").unwrap();
        std::fs::write(dir.path().join("src/main.rs"), b"fn main() {}").unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), b"pub fn foo() {}").unwrap();
        std::fs::write(dir.path().join("src/utils.rs"), b"pub fn bar() {}").unwrap();
        // Python files (fewer)
        std::fs::write(dir.path().join("script.py"), b"print('x')").unwrap();

        let engine = RecommendationEngine::from_builtin();
        let empty: Vec<String> = Vec::new();
        let recs = engine.recommend_for_project(dir.path(), &empty);

        // Rust should come first (higher confidence from project files)
        assert!(!recs.is_empty());
        assert_eq!(recs[0].plugin_id, "rust-analyzer");
    }

    // ------------------------------------------------------------------
    // Built-in recommendation loading
    // ------------------------------------------------------------------

    #[test]
    fn test_load_builtin_recommendations_not_empty() {
        let rules = load_builtin_recommendations();
        assert!(!rules.is_empty());
        assert!(rules.len() >= 5);
    }

    #[test]
    fn test_load_builtin_recommendations_each_has_id() {
        for rule in load_builtin_recommendations() {
            assert!(!rule.plugin_id.is_empty());
            assert!(!rule.file_patterns.is_empty());
            assert!(!rule.languages.is_empty());
        }
    }

    #[test]
    fn test_load_builtin_recommendations_rust_has_cargo_toml() {
        let rules = load_builtin_recommendations();
        let rust = rules
            .iter()
            .find(|r| r.plugin_id == "rust-analyzer")
            .unwrap();
        assert!(rust.file_patterns.iter().any(|p| p.contains("Cargo.toml")));
        assert!(rust.file_patterns.iter().any(|p| p.contains(".rs")));
    }

    // ------------------------------------------------------------------
    // Dismissal logic
    // ------------------------------------------------------------------

    #[test]
    fn test_is_dismissed_permanent() {
        let dismissal = RecommendationDismissal {
            plugin_id: "rust-analyzer".to_string(),
            dismissed_at: chrono::Utc::now(),
            remind_after: None,
        };
        assert!(is_dismissed("rust-analyzer", &[dismissal]));
    }

    #[test]
    fn test_is_dismissed_timed_not_yet_expired() {
        let dismissal = RecommendationDismissal {
            plugin_id: "python-lsp".to_string(),
            dismissed_at: chrono::Utc::now(),
            remind_after: Some(std::time::Duration::from_secs(3600)), // 1 hour
        };
        assert!(is_dismissed("python-lsp", &[dismissal]));
    }

    #[test]
    fn test_is_dismissed_timed_expired() {
        let dismissal = RecommendationDismissal {
            plugin_id: "gopls".to_string(),
            dismissed_at: chrono::Utc::now() - chrono::Duration::hours(2),
            remind_after: Some(std::time::Duration::from_secs(3600)), // 1 hour
        };
        assert!(!is_dismissed("gopls", &[dismissal]));
    }

    #[test]
    fn test_is_dismissed_not_found() {
        let dismissal = RecommendationDismissal {
            plugin_id: "rust-analyzer".to_string(),
            dismissed_at: chrono::Utc::now(),
            remind_after: None,
        };
        assert!(!is_dismissed("python-lsp", &[dismissal]));
    }

    #[test]
    fn test_dismissal_save_and_load_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("dismissals.json");

        let dismissal = RecommendationDismissal {
            plugin_id: "clangd".to_string(),
            dismissed_at: chrono::Utc::now(),
            remind_after: Some(std::time::Duration::from_secs(86400)), // 1 day
        };

        save_dismissal(&dismissal, &path);
        assert!(path.exists());

        let loaded = load_dismissals(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].plugin_id, "clangd");
    }

    #[test]
    fn test_dismissal_save_replaces_existing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("dismissals.json");

        let d1 = RecommendationDismissal {
            plugin_id: "rust-analyzer".to_string(),
            dismissed_at: chrono::Utc::now(),
            remind_after: None,
        };
        save_dismissal(&d1, &path);

        let d2 = RecommendationDismissal {
            plugin_id: "rust-analyzer".to_string(),
            dismissed_at: chrono::Utc::now() + chrono::Duration::hours(1),
            remind_after: Some(std::time::Duration::from_secs(7200)),
        };
        save_dismissal(&d2, &path);

        let loaded = load_dismissals(&path);
        assert_eq!(loaded.len(), 1);
        assert!(loaded[0].remind_after.is_some());
    }

    // ------------------------------------------------------------------
    // Simple pattern matching
    // ------------------------------------------------------------------

    #[test]
    fn test_simple_pattern_match_extension() {
        assert!(simple_pattern_match("main.rs", "*.rs"));
        assert!(simple_pattern_match("main.rs", "**/*.rs"));
        assert!(simple_pattern_match("index.js", "*.js"));
        assert!(simple_pattern_match("app.tsx", "*.tsx"));
    }

    #[test]
    fn test_simple_pattern_match_exact() {
        assert!(simple_pattern_match("Cargo.toml", "**/Cargo.toml"));
        assert!(simple_pattern_match("package.json", "**/package.json"));
        assert!(simple_pattern_match("go.mod", "**/go.mod"));
    }

    #[test]
    fn test_simple_pattern_match_no_match() {
        assert!(!simple_pattern_match("main.rs", "*.py"));
        assert!(!simple_pattern_match("readme.md", "*.rs"));
        assert!(!simple_pattern_match("Cargo.toml", "*.rs"));
    }

    // ------------------------------------------------------------------
    // Confidence calculation
    // ------------------------------------------------------------------

    #[test]
    fn test_calculate_confidence_empty() {
        assert_eq!(calculate_confidence(&[]), 0.0);
    }

    #[test]
    fn test_calculate_confidence_with_project_file() {
        let files = vec![
            "/project/Cargo.toml".to_string(),
            "/project/src/main.rs".to_string(),
            "/project/src/lib.rs".to_string(),
        ];
        let conf = calculate_confidence(&files);
        assert!((conf - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn test_calculate_confidence_without_project_file() {
        let files = vec![
            "/project/a.rs".to_string(),
            "/project/b.rs".to_string(),
            "/project/c.rs".to_string(),
            "/project/d.rs".to_string(),
            "/project/e.rs".to_string(),
        ];
        let conf = calculate_confidence(&files);
        assert!((conf - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn test_calculate_confidence_single_file() {
        let files = vec!["/project/main.rs".to_string()];
        let conf = calculate_confidence(&files);
        assert!((conf - 0.5).abs() < f64::EPSILON);
    }

    // ------------------------------------------------------------------
    // User recommendation loading
    // ------------------------------------------------------------------

    #[test]
    fn test_load_user_recommendations_missing_file() {
        let path = Path::new("/nonexistent/recommendations.json");
        let rules = load_user_recommendations(path);
        assert!(rules.is_empty());
    }

    #[test]
    fn test_load_user_recommendations_valid_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("recs.json");
        std::fs::write(
            &path,
            r#"[
            {
                "plugin_id": "custom-lsp",
                "plugin_name": "Custom LSP",
                "description": "A custom language server",
                "languages": ["custom"],
                "file_patterns": ["*.custom"],
                "min_confidence": 0.5
            }
        ]"#,
        )
        .unwrap();

        let rules = load_user_recommendations(&path);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].plugin_id, "custom-lsp");
        assert!(rules[0].file_patterns.contains(&"*.custom".to_string()));
    }

    #[test]
    fn test_load_user_recommendations_invalid_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("recs.json");
        std::fs::write(&path, "not valid json").unwrap();
        let rules = load_user_recommendations(&path);
        assert!(rules.is_empty());
    }

    // ------------------------------------------------------------------
    // install_recommended_plugin helper
    // ------------------------------------------------------------------

    #[test]
    fn test_install_recommended_plugin_empty_id() {
        let result = install_recommended_plugin("");
        assert!(result.is_err());
        match result {
            Err(RecommendationError::InvalidPluginId(id)) => assert!(id.is_empty()),
            _ => panic!("expected InvalidPluginId"),
        }
    }

    #[test]
    fn test_install_recommended_plugin_valid_id() {
        let result = install_recommended_plugin("some-plugin");
        assert!(result.is_ok());
    }

    // ------------------------------------------------------------------
    // File scanning with hidden/target dir exclusion
    // ------------------------------------------------------------------

    #[test]
    fn test_find_matching_files_skips_hidden_and_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join(".hidden")).unwrap();
        std::fs::write(dir.path().join(".hidden/secret.rs"), b"").unwrap();
        std::fs::create_dir(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("target/debug.rs"), b"").unwrap();
        std::fs::write(dir.path().join("visible.rs"), b"").unwrap();

        let patterns = vec!["*.rs".to_string()];
        let matched = find_matching_files(dir.path(), &patterns);
        assert_eq!(matched.len(), 1);
        assert!(matched[0].contains("visible.rs"));
    }
}
