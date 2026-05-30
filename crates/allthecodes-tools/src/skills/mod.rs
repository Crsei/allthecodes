use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::common::{string_param, validate_enum};
use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, Tools, ValidationResult};
use allthecodes_types::message::AssistantMessage;

pub fn tools() -> Tools {
    vec![Arc::new(DiscoverSkillsTool)]
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

fn source_matches(source: &allthecodes_skills::SkillSource, filter: &str) -> bool {
    match filter {
        "all" => true,
        "bundled" => matches!(source, allthecodes_skills::SkillSource::Bundled),
        "user" => matches!(source, allthecodes_skills::SkillSource::User),
        "project" => matches!(source, allthecodes_skills::SkillSource::Project),
        "plugin" => matches!(source, allthecodes_skills::SkillSource::Plugin(_)),
        "mcp" => matches!(source, allthecodes_skills::SkillSource::Mcp(_)),
        _ => false,
    }
}

fn text_score(query: &str, fields: &[&str]) -> usize {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return 1;
    }
    let terms = query
        .split_whitespace()
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return 0;
    }
    let field_text = fields.join("\n").to_ascii_lowercase();
    let field_tokens = fields
        .iter()
        .flat_map(|field| search_identifier_tokens(field))
        .collect::<HashSet<_>>();
    let mut score = 0;
    if fields
        .iter()
        .any(|field| field.eq_ignore_ascii_case(&query))
    {
        score += 500;
    }
    for term in terms {
        if let Some(required) = term.strip_prefix('+') {
            if !field_text.contains(required) && !field_tokens.contains(required) {
                return 0;
            }
            score += 30;
        } else if field_tokens.contains(term) {
            score += 60;
        } else if field_text.contains(term) {
            score += 10;
        }
    }
    score
}

fn search_identifier_tokens(input: &str) -> Vec<String> {
    let mut normalized = String::with_capacity(input.len() * 2);
    let mut prev_lower_or_digit = false;
    for ch in input.chars() {
        if ch.is_ascii_uppercase() && prev_lower_or_digit {
            normalized.push(' ');
        }
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            prev_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        } else if ch == '_' || ch == '-' || ch == '.' || ch == '/' || ch == ':' {
            normalized.push(' ');
            prev_lower_or_digit = false;
        } else {
            normalized.push(ch);
            prev_lower_or_digit = false;
        }
    }
    normalized
        .split_whitespace()
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub struct DiscoverSkillsTool;

fn skill_query(input: &Value) -> &str {
    string_param(input, "query")
        .or_else(|| string_param(input, "description"))
        .unwrap_or("")
}

fn skill_result_limit(input: &Value) -> usize {
    input
        .get("max_results")
        .or_else(|| input.get("limit"))
        .and_then(Value::as_u64)
        .unwrap_or(25)
        .clamp(1, 100) as usize
}

#[async_trait]
impl Tool for DiscoverSkillsTool {
    fn name(&self) -> &str {
        "DiscoverSkills"
    }

    async fn description(&self, _input: &Value) -> String {
        "Discover bundled, user, project, plugin, and MCP skills available to this session.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Optional text query over skill names, descriptions, and usage hints."},
                "description": {"type": "string", "description": "Compatibility alias for query."},
                "source": {"type": "string", "enum": ["bundled", "user", "project", "plugin", "mcp", "all"], "description": "Skill source filter."},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 100},
                "limit": {"type": "integer", "minimum": 1, "maximum": 100, "description": "Compatibility alias for max_results."}
            }
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if let Some(result) = validate_enum(
            input,
            "source",
            &["bundled", "user", "project", "plugin", "mcp", "all"],
        ) {
            return result;
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let query = skill_query(&input);
        let source_filter = string_param(&input, "source").unwrap_or("all");
        let max_results = skill_result_limit(&input);

        let mut skills = allthecodes_skills::get_all_skills();
        if skills.is_empty() {
            skills = allthecodes_skills::bundled::bundled_skills();
        }

        let mut matches = skills
            .into_iter()
            .filter(|skill| source_matches(&skill.source, source_filter))
            .filter_map(|skill| {
                let source = source_label(&skill.source);
                let allowed_tools = skill.frontmatter.allowed_tools.join(" ");
                let argument_names = skill.frontmatter.argument_names.join(" ");
                let paths = skill.frontmatter.paths.join(" ");
                let assets = skill.frontmatter.assets.join(" ");
                let entry_docs = skill.frontmatter.entry_docs.join(" ");
                let dependencies = skill
                    .frontmatter
                    .dependencies
                    .iter()
                    .map(allthecodes_skills::SkillDependency::label)
                    .collect::<Vec<_>>()
                    .join(" ");
                let score = text_score(
                    query,
                    &[
                        &skill.name,
                        skill.display_name(),
                        &source,
                        &skill.frontmatter.description,
                        skill.frontmatter.when_to_use.as_deref().unwrap_or(""),
                        skill.frontmatter.argument_hint.as_deref().unwrap_or(""),
                        skill.frontmatter.agent.as_deref().unwrap_or(""),
                        &allowed_tools,
                        &argument_names,
                        &paths,
                        &assets,
                        &entry_docs,
                        &dependencies,
                        &skill.prompt_body,
                    ],
                );
                if score == 0 {
                    return None;
                }
                Some((
                    score,
                    json!({
                        "name": skill.name,
                        "display_name": skill.display_name(),
                        "source": source,
                        "description": skill.frontmatter.description,
                        "when_to_use": skill.frontmatter.when_to_use,
                        "allowed_tools": skill.frontmatter.allowed_tools,
                        "user_invocable": skill.is_user_invocable(),
                        "model_invocable": skill.is_model_invocable(),
                        "context": skill.frontmatter.context,
                        "agent": skill.frontmatter.agent,
                        "version": skill.effective_version(),
                    }),
                ))
            })
            .collect::<Vec<_>>();

        matches.sort_by(|a, b| {
            b.0.cmp(&a.0).then_with(|| {
                a.1["name"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b.1["name"].as_str().unwrap_or(""))
            })
        });
        matches.truncate(max_results);

        Ok(ToolResult {
            data: json!({
                "query": query,
                "source": source_filter,
                "count": matches.len(),
                "skills": matches.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            }),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Discover available skills by name, source, description, and when-to-use metadata.".into()
    }
}
