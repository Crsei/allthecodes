//! Skill directory loader: discovers and parses `SKILL.md` package files.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::{
    SkillContext, SkillDefinition, SkillDependency, SkillDiagnostic, SkillFrontmatter, SkillSource,
};

const KNOWN_FRONTMATTER_KEYS: &[&str] = &[
    "name",
    "description",
    "when-to-use",
    "when_to_use",
    "whenToUse",
    "allowed-tools",
    "allowed_tools",
    "allowedTools",
    "argument-hint",
    "argument_hint",
    "argumentHint",
    "arguments",
    "model",
    "user-invocable",
    "user_invocable",
    "userInvocable",
    "disable-model-invocation",
    "disable_model_invocation",
    "disableModelInvocation",
    "context",
    "agent",
    "effort",
    "version",
    "compatible-app-version",
    "compatible_app_version",
    "compatibleAppVersion",
    "dependencies",
    "paths",
    "assets",
    "entry-docs",
    "entry_docs",
    "entryDocs",
];

#[derive(Debug, Clone, Default)]
pub struct SkillDirectoryLoad {
    pub skills: Vec<SkillDefinition>,
    pub diagnostics: Vec<SkillDiagnostic>,
    pub skipped: usize,
}

// ---------------------------------------------------------------------------
// Frontmatter parsing
// ---------------------------------------------------------------------------

/// Parse YAML-ish frontmatter from a markdown string.
///
/// Returns `(frontmatter_map, body_after_frontmatter)`. This compatibility
/// helper keeps the old lossy signature; loading paths use the diagnostic
/// parser below.
pub fn parse_frontmatter(content: &str) -> (HashMap<String, String>, String) {
    let parsed = parse_frontmatter_diagnostic(content, None);
    (parsed.map, parsed.body)
}

#[derive(Debug, Clone)]
struct ParsedFrontmatter {
    map: HashMap<String, String>,
    body: String,
    diagnostics: Vec<SkillDiagnostic>,
}

fn parse_frontmatter_diagnostic(content: &str, path: Option<&Path>) -> ParsedFrontmatter {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return ParsedFrontmatter {
            map: HashMap::new(),
            body: content.to_string(),
            diagnostics: Vec::new(),
        };
    }

    let after_open = &trimmed[3..];
    let Some(close_pos) = after_open.find("\n---") else {
        let mut diagnostic = SkillDiagnostic::error(
            "malformed-frontmatter",
            "Opening frontmatter fence has no closing '---'.",
        );
        if let Some(path) = path {
            diagnostic = diagnostic.with_path(path);
        }
        return ParsedFrontmatter {
            map: HashMap::new(),
            body: content.to_string(),
            diagnostics: vec![diagnostic],
        };
    };

    let yaml_block = &after_open[..close_pos];
    let body = after_open[close_pos + 4..].trim_start_matches('\n');

    let mut map = HashMap::new();
    let mut diagnostics = Vec::new();
    let known: HashSet<&str> = KNOWN_FRONTMATTER_KEYS.iter().copied().collect();

    for (idx, raw_line) in yaml_block.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            let mut diagnostic = SkillDiagnostic::error(
                "malformed-frontmatter-line",
                format!("Frontmatter line {} is missing ':' separator.", idx + 1),
            );
            if let Some(path) = path {
                diagnostic = diagnostic.with_path(path);
            }
            diagnostics.push(diagnostic);
            continue;
        };

        let key = key.trim().to_string();
        if !known.contains(key.as_str()) {
            let mut diagnostic = SkillDiagnostic::warning(
                "unknown-frontmatter-key",
                format!("Unknown skill frontmatter key '{}'.", key),
            );
            if let Some(path) = path {
                diagnostic = diagnostic.with_path(path);
            }
            diagnostics.push(diagnostic);
        }

        let normalized_key = normalize_key(&key);
        let value = strip_quotes(value.trim()).to_string();
        map.insert(normalized_key, value);
    }

    ParsedFrontmatter {
        map,
        body: body.to_string(),
        diagnostics,
    }
}

fn normalize_key(key: &str) -> String {
    match key {
        "whenToUse" => "when-to-use".to_string(),
        "allowedTools" => "allowed-tools".to_string(),
        "argumentHint" => "argument-hint".to_string(),
        "userInvocable" => "user-invocable".to_string(),
        "disableModelInvocation" => "disable-model-invocation".to_string(),
        "compatibleAppVersion" => "compatible-app-version".to_string(),
        "entryDocs" => "entry-docs".to_string(),
        other => other.trim().to_lowercase().replace(['_', ' '], "-"),
    }
}

fn strip_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value)
}

/// Parse frontmatter map into structured [`SkillFrontmatter`].
pub fn parse_skill_frontmatter(fm: &HashMap<String, String>, body: &str) -> SkillFrontmatter {
    let description = fm
        .get("description")
        .cloned()
        .unwrap_or_else(|| extract_first_paragraph(body));

    let when_to_use = fm.get("when-to-use").cloned();
    let allowed_tools = fm
        .get("allowed-tools")
        .map(|s| parse_list(s))
        .unwrap_or_default();

    let argument_hint = fm.get("argument-hint").cloned();
    let argument_names = fm
        .get("arguments")
        .map(|s| parse_list(s))
        .unwrap_or_default();
    let model = fm.get("model").cloned();

    let user_invocable = fm
        .get("user-invocable")
        .map(|v| parse_bool(v).unwrap_or(true))
        .unwrap_or(true);

    let disable_model_invocation = fm
        .get("disable-model-invocation")
        .map(|v| parse_bool(v).unwrap_or(false))
        .unwrap_or(false);

    let context = fm
        .get("context")
        .map(|v| {
            if v.eq_ignore_ascii_case("fork") {
                SkillContext::Fork
            } else {
                SkillContext::Inline
            }
        })
        .unwrap_or_default();

    SkillFrontmatter {
        name: fm.get("name").cloned(),
        description,
        when_to_use,
        allowed_tools,
        argument_hint,
        argument_names,
        model,
        user_invocable,
        disable_model_invocation,
        context,
        agent: fm.get("agent").cloned(),
        effort: fm.get("effort").cloned(),
        version: fm.get("version").cloned(),
        compatible_app_version: fm.get("compatible-app-version").cloned(),
        dependencies: fm
            .get("dependencies")
            .map(|s| parse_dependencies(s))
            .unwrap_or_default(),
        paths: fm.get("paths").map(|s| parse_list(s)).unwrap_or_default(),
        assets: fm.get("assets").map(|s| parse_list(s)).unwrap_or_default(),
        entry_docs: fm
            .get("entry-docs")
            .map(|s| parse_list(s))
            .unwrap_or_default(),
    }
}

fn parse_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" | "on" => Some(true),
        "false" | "no" | "0" | "off" => Some(false),
        _ => None,
    }
}

fn parse_list(raw: &str) -> Vec<String> {
    let mut s = raw.trim();
    if let Some(stripped) = s.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        s = stripped;
    }

    s.split([',', '\n'])
        .flat_map(|chunk| {
            if chunk.contains(' ') && !chunk.contains('/') && !chunk.contains('\\') {
                chunk
                    .split_whitespace()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            } else {
                vec![chunk.to_string()]
            }
        })
        .map(|p| strip_quotes(p.trim()).to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn parse_dependencies(raw: &str) -> Vec<SkillDependency> {
    let mut s = raw.trim();
    if let Some(stripped) = s.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        s = stripped;
    }

    s.split([',', '\n'])
        .map(|item| strip_quotes(item.trim()).to_string())
        .filter_map(|item| {
            let item = item.trim();
            if item.is_empty() {
                return None;
            }
            if let Some((name, req)) = item.split_once(char::is_whitespace) {
                let req = req.trim();
                if !req.is_empty() {
                    return Some(SkillDependency::new(name.trim(), Some(req.to_string())));
                }
            }
            for op in [">=", "<=", ">", "<", "=", "^", "~"] {
                if let Some((name, req)) = item.split_once(op) {
                    let req = format!("{}{}", op, req.trim());
                    return Some(SkillDependency::new(
                        name.trim().trim_end_matches('@'),
                        Some(req),
                    ));
                }
            }
            if let Some((name, version)) = item.split_once('@') {
                let version = version.trim();
                let version = if version.is_empty() {
                    None
                } else {
                    Some(version.to_string())
                };
                return Some(SkillDependency::new(name.trim(), version));
            }
            Some(SkillDependency::new(item, None))
        })
        .collect()
}

/// Extract the first non-empty paragraph from markdown as a fallback description.
fn extract_first_paragraph(body: &str) -> String {
    for line in body.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            return trimmed.to_string();
        }
    }
    String::new()
}

// ---------------------------------------------------------------------------
// Directory scanning
// ---------------------------------------------------------------------------

/// Load all skills from a directory.
///
/// Expects the layout: `dir/skill-name/SKILL.md`.
pub fn load_skills_from_dir(dir: &Path, source: SkillSource) -> Vec<SkillDefinition> {
    load_skills_from_dir_with_diagnostics(dir, source).skills
}

/// Load all skills from a directory with actionable diagnostics.
pub fn load_skills_from_dir_with_diagnostics(
    dir: &Path,
    source: SkillSource,
) -> SkillDirectoryLoad {
    let mut load = SkillDirectoryLoad::default();

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            load.diagnostics.push(
                SkillDiagnostic::error(
                    "skills-dir-read-failed",
                    format!("Failed to read skills directory '{}': {}", dir.display(), e),
                )
                .with_path(dir),
            );
            return load;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let skill_file = path.join("SKILL.md");
        let skill_file = if skill_file.is_file() {
            skill_file
        } else {
            let lower = path.join("skill.md");
            if lower.is_file() {
                lower
            } else {
                continue;
            }
        };

        match load_skill_file_with_diagnostics(&skill_file, &path, &source) {
            Some((skill, diagnostics)) => {
                load.skills.push(skill);
                load.diagnostics.extend(diagnostics);
            }
            None => {
                load.skipped += 1;
                load.diagnostics.push(
                    SkillDiagnostic::error(
                        "skill-file-read-failed",
                        format!("Failed to load skill file '{}'.", skill_file.display()),
                    )
                    .with_source(source.clone())
                    .with_path(skill_file),
                );
            }
        }
    }

    load
}

/// Load legacy commands from `.allthecodes/commands/` directory.
pub fn load_legacy_commands(dir: &Path, source: SkillSource) -> Vec<SkillDefinition> {
    let mut skills = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return skills,
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            let skill_file = path.join("SKILL.md");
            if skill_file.is_file() {
                if let Some(skill) = load_skill_file(&skill_file, &path, &source) {
                    skills.push(skill);
                }
                continue;
            }
        }

        if path.is_file() && path.extension().map(|ext| ext == "md").unwrap_or(false) {
            if let Some(skill) = load_skill_file(&path, path.parent().unwrap_or(&path), &source) {
                skills.push(skill);
            }
        }
    }

    skills
}

fn load_skill_file(
    file_path: &Path,
    skill_dir: &Path,
    source: &SkillSource,
) -> Option<SkillDefinition> {
    load_skill_file_with_diagnostics(file_path, skill_dir, source).map(|(skill, _)| skill)
}

fn load_skill_file_with_diagnostics(
    file_path: &Path,
    skill_dir: &Path,
    source: &SkillSource,
) -> Option<(SkillDefinition, Vec<SkillDiagnostic>)> {
    let content = std::fs::read_to_string(file_path).ok()?;
    let parsed = parse_frontmatter_diagnostic(&content, Some(file_path));
    let frontmatter = parse_skill_frontmatter(&parsed.map, &parsed.body);

    let name = frontmatter.name.clone().unwrap_or_else(|| {
        if file_path
            .file_name()
            .map(|f| f == "SKILL.md" || f == "skill.md")
            .unwrap_or(false)
        {
            skill_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unnamed".to_string())
        } else {
            file_path
                .file_stem()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unnamed".to_string())
        }
    });

    let diagnostics = parsed
        .diagnostics
        .into_iter()
        .map(|d| d.with_skill(name.clone()).with_source(source.clone()))
        .collect();

    Some((
        SkillDefinition {
            name,
            source: source.clone(),
            base_dir: Some(skill_dir.to_path_buf()),
            frontmatter,
            prompt_body: parsed.body,
        },
        diagnostics,
    ))
}

/// Load a single skill from an explicit markdown file path.
pub fn load_skill_from_file_path(file_path: &Path, source: SkillSource) -> Option<SkillDefinition> {
    let skill_dir = file_path.parent()?;
    load_skill_file(file_path, skill_dir, &source)
}

/// Load a single skill from explicit markdown content.
pub fn load_skill_from_content(
    content: &str,
    name: &str,
    source: SkillSource,
) -> (SkillDefinition, Vec<SkillDiagnostic>) {
    let parsed = parse_frontmatter_diagnostic(content, None);
    let mut frontmatter = parse_skill_frontmatter(&parsed.map, &parsed.body);
    if frontmatter.name.is_none() {
        frontmatter.name = Some(name.to_string());
    }

    let diagnostics = parsed
        .diagnostics
        .into_iter()
        .map(|d| d.with_skill(name.to_string()).with_source(source.clone()))
        .collect();

    (
        SkillDefinition {
            name: name.to_string(),
            source,
            base_dir: None,
            frontmatter,
            prompt_body: parsed.body,
        },
        diagnostics,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("allthecodes_skills_{}_{}", name, nanos))
    }

    #[test]
    fn test_parse_frontmatter_basic() {
        let content = r#"---
description: A test skill
allowed-tools: Read, Grep, Bash
user-invocable: true
---

Do something useful."#;

        let (fm, body) = parse_frontmatter(content);
        assert_eq!(fm.get("description").unwrap(), "A test skill");
        assert_eq!(fm.get("allowed-tools").unwrap(), "Read, Grep, Bash");
        assert_eq!(fm.get("user-invocable").unwrap(), "true");
        assert!(body.contains("Do something useful."));
    }

    #[test]
    fn test_parse_frontmatter_no_frontmatter() {
        let content = "Just plain markdown body.";
        let (fm, body) = parse_frontmatter(content);
        assert!(fm.is_empty());
        assert_eq!(body, content);
    }

    #[test]
    fn test_parse_frontmatter_quoted_values() {
        let content = "---\nname: \"My Skill\"\n---\nBody";
        let (fm, _body) = parse_frontmatter(content);
        assert_eq!(fm.get("name").unwrap(), "My Skill");
    }

    #[test]
    fn test_parse_skill_frontmatter() {
        let mut fm = HashMap::new();
        fm.insert("description".to_string(), "Test desc".to_string());
        fm.insert("allowed-tools".to_string(), "Read,Grep".to_string());
        fm.insert("context".to_string(), "fork".to_string());
        fm.insert("disable-model-invocation".to_string(), "true".to_string());
        fm.insert(
            "dependencies".to_string(),
            "base >=1.0.0, helper".to_string(),
        );

        let sf = parse_skill_frontmatter(&fm, "body");
        assert_eq!(sf.description, "Test desc");
        assert_eq!(sf.allowed_tools, vec!["Read", "Grep"]);
        assert_eq!(sf.context, SkillContext::Fork);
        assert!(sf.disable_model_invocation);
        assert!(sf.user_invocable);
        assert_eq!(sf.dependencies.len(), 2);
        assert_eq!(sf.dependencies[0].name, "base");
        assert_eq!(sf.dependencies[0].version.as_deref(), Some(">=1.0.0"));
    }

    #[test]
    fn test_parse_skill_frontmatter_defaults() {
        let fm = HashMap::new();
        let sf = parse_skill_frontmatter(&fm, "First paragraph here.\n\nSecond.");
        assert_eq!(sf.description, "First paragraph here.");
        assert!(sf.user_invocable);
        assert!(!sf.disable_model_invocation);
        assert_eq!(sf.context, SkillContext::Inline);
    }

    #[test]
    fn test_parse_paths_frontmatter() {
        let mut fm = HashMap::new();
        fm.insert("paths".to_string(), "[\"src/**\", \"lib/**\"]".to_string());
        let sf = parse_skill_frontmatter(&fm, "");
        assert_eq!(sf.paths, vec!["src/**", "lib/**"]);
    }

    #[test]
    fn test_extract_first_paragraph() {
        assert_eq!(extract_first_paragraph("# Title\nParagraph"), "Paragraph");
        assert_eq!(extract_first_paragraph("Immediate text"), "Immediate text");
        assert_eq!(extract_first_paragraph(""), "");
    }

    #[test]
    fn test_load_skills_from_dir_empty() {
        let dir = temp_path("empty");
        let _ = std::fs::create_dir_all(&dir);
        let skills = load_skills_from_dir(&dir, SkillSource::User);
        assert!(skills.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_skill_from_temp_dir() {
        let base = temp_path("load");
        let skill_dir = base.join("my-skill");
        let _ = std::fs::create_dir_all(&skill_dir);

        let skill_file = skill_dir.join("SKILL.md");
        let mut f = std::fs::File::create(&skill_file).unwrap();
        writeln!(
            f,
            "---\ndescription: My test skill\nallowed-tools: Read, Bash\nversion: 1.2.3\n---\n\nDo the thing."
        )
        .unwrap();

        let load = load_skills_from_dir_with_diagnostics(&base, SkillSource::User);
        assert_eq!(load.skills.len(), 1);
        assert_eq!(load.skills[0].name, "my-skill");
        assert_eq!(load.skills[0].frontmatter.description, "My test skill");
        assert_eq!(
            load.skills[0].frontmatter.allowed_tools,
            vec!["Read", "Bash"]
        );
        assert_eq!(load.skills[0].frontmatter.version.as_deref(), Some("1.2.3"));
        assert!(load.skills[0].prompt_body.contains("Do the thing."));
        assert!(load.diagnostics.is_empty());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_directory_reload_add_update_delete() {
        let base = temp_path("reload");
        let skill_dir = base.join("reloadable");
        let _ = std::fs::create_dir_all(&skill_dir);

        let initial = load_skills_from_dir_with_diagnostics(&base, SkillSource::User);
        assert!(initial.skills.is_empty());

        let skill_file = skill_dir.join("SKILL.md");
        std::fs::write(
            &skill_file,
            "---\ndescription: Reloadable skill\nversion: 1.0.0\n---\n\nBody.",
        )
        .unwrap();
        let added = load_skills_from_dir_with_diagnostics(&base, SkillSource::User);
        assert_eq!(added.skills.len(), 1);
        assert_eq!(
            added.skills[0].frontmatter.version.as_deref(),
            Some("1.0.0")
        );

        std::fs::write(
            &skill_file,
            "---\ndescription: Reloadable skill\nversion: 2.0.0\n---\n\nUpdated body.",
        )
        .unwrap();
        let updated = load_skills_from_dir_with_diagnostics(&base, SkillSource::User);
        assert_eq!(updated.skills.len(), 1);
        assert_eq!(
            updated.skills[0].frontmatter.version.as_deref(),
            Some("2.0.0")
        );
        assert!(updated.skills[0].prompt_body.contains("Updated body"));

        std::fs::remove_dir_all(&skill_dir).unwrap();
        let deleted = load_skills_from_dir_with_diagnostics(&base, SkillSource::User);
        assert!(deleted.skills.is_empty());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_load_legacy_command_single_file() {
        let base = temp_path("legacy");
        let _ = std::fs::create_dir_all(&base);

        let cmd_file = base.join("greet.md");
        let mut f = std::fs::File::create(&cmd_file).unwrap();
        writeln!(f, "---\ndescription: Greet someone\n---\n\nSay hello.").unwrap();

        let skills = load_legacy_commands(&base, SkillSource::User);
        assert!(skills.iter().any(|s| s.name == "greet"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn malformed_frontmatter_reports_diagnostic() {
        let parsed = parse_frontmatter_diagnostic("---\ndescription test\nBody", None);
        assert!(parsed
            .diagnostics
            .iter()
            .any(|d| d.code == "malformed-frontmatter"));
    }

    #[test]
    fn unknown_frontmatter_key_warns() {
        let parsed = parse_frontmatter_diagnostic("---\nwat: nope\n---\nBody", None);
        assert!(parsed
            .diagnostics
            .iter()
            .any(|d| d.code == "unknown-frontmatter-key"));
    }
}
