use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::common::{string_param, validate_enum};
use crate::discovery_search::{
    DiscoveryNextAction, DiscoveryResultKind, DiscoverySearchError, DiscoverySearchInput,
    DiscoverySearchOutput, DiscoverySearchResult, DiscoverySignal, DiscoverySkillSummary,
    DiscoveryStatusSummary,
};
use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, Tools, ValidationResult};
use allthecodes_config::features::{self, Feature};
use allthecodes_types::message::AssistantMessage;

pub fn tools() -> Tools {
    vec![Arc::new(DiscoverSkillsTool), Arc::new(SkillSearchTool)]
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
pub struct SkillSearchTool;

#[derive(Debug, Clone)]
struct ScoredSkillRow {
    score: usize,
    source: String,
    match_reasons: Vec<String>,
    skill: allthecodes_skills::SkillDefinition,
}

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

fn available_skills() -> Vec<allthecodes_skills::SkillDefinition> {
    let mut skills = allthecodes_skills::get_all_skills();
    if skills.is_empty() {
        skills = allthecodes_skills::bundled::bundled_skills();
    }
    skills
}

fn search_skill_rows(query: &str, source_filter: &str, max_results: usize) -> Vec<ScoredSkillRow> {
    let mut matches = available_skills()
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
            let fields = [
                skill.name.as_str(),
                skill.display_name(),
                source.as_str(),
                skill.frontmatter.description.as_str(),
                skill.frontmatter.when_to_use.as_deref().unwrap_or(""),
                skill.frontmatter.argument_hint.as_deref().unwrap_or(""),
                skill.frontmatter.agent.as_deref().unwrap_or(""),
                allowed_tools.as_str(),
                argument_names.as_str(),
                paths.as_str(),
                assets.as_str(),
                entry_docs.as_str(),
                dependencies.as_str(),
                skill.prompt_body.as_str(),
            ];
            let score = text_score(query, &fields);
            if score == 0 {
                return None;
            }
            Some(ScoredSkillRow {
                score,
                source,
                match_reasons: skill_match_reasons(query, &skill),
                skill,
            })
        })
        .collect::<Vec<_>>();

    matches.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.skill.name.cmp(&b.skill.name))
    });
    matches.truncate(max_results);
    matches
}

fn skill_match_reasons(query: &str, skill: &allthecodes_skills::SkillDefinition) -> Vec<String> {
    let mut reasons = Vec::new();
    let source = source_label(&skill.source);
    for (reason, field) in [
        ("name", skill.name.as_str()),
        ("display_name", skill.display_name()),
        ("source", source.as_str()),
        ("description", skill.frontmatter.description.as_str()),
        (
            "when_to_use",
            skill.frontmatter.when_to_use.as_deref().unwrap_or(""),
        ),
        ("prompt", skill.prompt_body.as_str()),
    ] {
        if field_matches_query(field, query) && !reasons.iter().any(|existing| existing == reason) {
            reasons.push(reason.to_string());
        }
    }
    if reasons.is_empty() {
        reasons.push("metadata".to_string());
    }
    reasons
}

fn field_matches_query(field: &str, query: &str) -> bool {
    let field_text = field.to_ascii_lowercase();
    let field_tokens = search_identifier_tokens(field)
        .into_iter()
        .collect::<HashSet<_>>();
    query
        .trim()
        .to_ascii_lowercase()
        .split_whitespace()
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .any(|term| field_text.contains(term) || field_tokens.contains(term))
}

fn legacy_skill_value(row: &ScoredSkillRow) -> Value {
    let skill = &row.skill;
    json!({
        "name": skill.name,
        "display_name": skill.display_name(),
        "source": row.source,
        "description": skill.frontmatter.description,
        "when_to_use": skill.frontmatter.when_to_use,
        "allowed_tools": skill.frontmatter.allowed_tools,
        "user_invocable": skill.is_user_invocable(),
        "model_invocable": skill.is_model_invocable(),
        "context": skill.frontmatter.context,
        "agent": skill.frontmatter.agent,
        "version": skill.effective_version(),
    })
}

pub(crate) fn run_discover_skills(input: &Value) -> anyhow::Result<Value> {
    let query = skill_query(input);
    let source_filter = string_param(input, "source").unwrap_or("all");
    let max_results = skill_result_limit(input);
    let matches = search_skill_rows(query, source_filter, max_results);

    Ok(json!({
        "query": query,
        "source": source_filter,
        "count": matches.len(),
        "skills": matches.iter().map(legacy_skill_value).collect::<Vec<_>>(),
    }))
}

pub fn run_skill_search(
    input: DiscoverySearchInput,
) -> std::result::Result<DiscoverySearchOutput, DiscoverySearchError> {
    if input.query.trim().is_empty() {
        return Err(DiscoverySearchError::new("query is required"));
    }
    let source_filter = input.source_filter.as_deref().unwrap_or("all");
    let matches = search_skill_rows(&input.query, source_filter, input.max_results.clamp(1, 100));
    let results = matches
        .iter()
        .map(skill_discovery_result)
        .collect::<Vec<_>>();

    Ok(DiscoverySearchOutput {
        data: json!({
            "query": input.query,
            "source": source_filter,
            "count": results.len(),
            "results": results,
        }),
        display_preview: None,
    })
}

pub fn mcp_skill_summary_results(
    query: &str,
    max_results: usize,
) -> std::result::Result<Vec<DiscoverySearchResult>, DiscoverySearchError> {
    if query.trim().is_empty() {
        return Err(DiscoverySearchError::new("query is required"));
    }
    Ok(search_skill_rows(query, "mcp", max_results.clamp(1, 100))
        .iter()
        .filter_map(mcp_skill_discovery_result)
        .collect())
}

fn skill_discovery_result(row: &ScoredSkillRow) -> DiscoverySearchResult {
    let skill = &row.skill;
    let mut result = DiscoverySearchResult::new(DiscoveryResultKind::Skill, skill.name.clone())
        .with_display_name(skill.display_name().to_string())
        .with_source(row.source.clone())
        .with_description(skill.frontmatter.description.clone())
        .with_invocation_flags(skill.is_user_invocable(), skill.is_model_invocable())
        .with_status(DiscoveryStatusSummary::new("available"))
        .with_next_action(DiscoveryNextAction::new(
            "Open skill",
            format!("/skills {}", skill.name),
        ))
        .with_signal(DiscoverySignal::ExplicitSearch);
    if let Some(when_to_use) = &skill.frontmatter.when_to_use {
        result = result.with_when_to_use(when_to_use.clone());
    }
    result = result.with_version(skill.effective_version().to_string());
    if let allthecodes_skills::SkillSource::Mcp(server) = &skill.source {
        result = result.with_server_name(server.clone());
    }
    if features::enabled(Feature::ExperimentalSkillSearch) {
        result = result.with_prefetch_source("local");
    }
    result.match_reasons = row.match_reasons.clone();
    result
}

fn mcp_skill_discovery_result(row: &ScoredSkillRow) -> Option<DiscoverySearchResult> {
    let skill = &row.skill;
    let allthecodes_skills::SkillSource::Mcp(server) = &skill.source else {
        return None;
    };
    let mut result = DiscoverySearchResult::new(DiscoveryResultKind::McpSkill, skill.name.clone())
        .with_source(row.source.clone())
        .with_server_name(server.clone())
        .with_description(skill.frontmatter.description.clone())
        .with_status(DiscoveryStatusSummary::new("available"))
        .with_skill_summaries([DiscoverySkillSummary::new(
            skill.name.clone(),
            skill.frontmatter.description.clone(),
        )
        .with_source(row.source.clone())])
        .with_next_action(DiscoveryNextAction::new(
            "Open MCP skill",
            format!("/skills {}", skill.name),
        ))
        .with_signal(DiscoverySignal::McpResourceDiscovery);
    result.match_reasons = row.match_reasons.clone();
    Some(result)
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
        Ok(ToolResult {
            data: run_discover_skills(&input)?,
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Discover available skills by name, source, description, and when-to-use metadata.".into()
    }
}

#[async_trait]
impl Tool for SkillSearchTool {
    fn name(&self) -> &str {
        "SkillSearch"
    }

    async fn description(&self, _input: &Value) -> String {
        "Search bundled, user, project, plugin, and MCP skills as discovery results.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Text query over skill names, descriptions, usage hints, sources, and prompt metadata."},
                "source": {"type": "string", "enum": ["bundled", "user", "project", "plugin", "mcp", "all"], "description": "Skill source filter."},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 100}
            },
            "required": ["query"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if string_param(input, "query")
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
        {
            return ValidationResult::Error {
                message: "query is required".to_string(),
                error_code: 400,
            };
        }
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
        let output = run_skill_search(DiscoverySearchInput {
            query: string_param(&input, "query").unwrap_or("").to_string(),
            source_filter: string_param(&input, "source").map(ToOwned::to_owned),
            max_results: skill_result_limit(&input),
            include_summaries: true,
        })?;
        Ok(ToolResult {
            data: output.data,
            display_preview: output.display_preview,
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Search available skills and return normalized discovery results. Use DiscoverSkills when legacy skills[] output is required.".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery_search::{DiscoveryResultKind, DiscoverySearchInput};
    use allthecodes_config::features::FeatureFlags;
    use allthecodes_skills::{SkillDefinition, SkillFrontmatter, SkillSource};
    use serial_test::serial;

    struct FeatureOverrideGuard;

    impl Drop for FeatureOverrideGuard {
        fn drop(&mut self) {
            allthecodes_config::features::clear_runtime_override();
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
        }
    }

    fn make_skill(name: &str, source: SkillSource, description: &str) -> SkillDefinition {
        SkillDefinition {
            name: name.to_string(),
            source,
            base_dir: None,
            frontmatter: SkillFrontmatter {
                name: Some(format!("{name} Display")),
                description: description.to_string(),
                when_to_use: Some(format!("Use {name} for review workflows")),
                version: Some("1.0.0".to_string()),
                user_invocable: true,
                ..Default::default()
            },
            prompt_body: format!("Prompt body for {name}"),
        }
    }

    #[test]
    #[serial]
    fn skill_search_filters_plugin_and_mcp_sources() {
        let _guard = SkillRegistryGuard::new(vec![
            make_skill(
                "review-plugin",
                SkillSource::Plugin("code-review".to_string()),
                "Review code with plugin metadata",
            ),
            make_skill(
                "linear-review",
                SkillSource::Mcp("linear".to_string()),
                "Review Linear issue context",
            ),
            make_skill(
                "bundled-review",
                SkillSource::Bundled,
                "Review bundled context",
            ),
        ]);

        let plugin = run_skill_search(DiscoverySearchInput {
            query: "review".to_string(),
            source_filter: Some("plugin".to_string()),
            max_results: 10,
            include_summaries: true,
        })
        .unwrap();
        assert_eq!(plugin.data["count"], 1);
        assert_eq!(plugin.data["results"][0]["source"], "plugin:code-review");

        let mcp = run_skill_search(DiscoverySearchInput {
            query: "linear".to_string(),
            source_filter: Some("mcp".to_string()),
            max_results: 10,
            include_summaries: true,
        })
        .unwrap();
        assert_eq!(mcp.data["count"], 1);
        assert_eq!(mcp.data["results"][0]["source"], "mcp:linear");
        assert_eq!(mcp.data["results"][0]["server_name"], "linear");
    }

    #[test]
    #[serial]
    fn discover_skills_preserves_description_alias_and_deterministic_ordering() {
        let _guard = SkillRegistryGuard::new(vec![
            make_skill("beta-database", SkillSource::Project, "database helper"),
            make_skill("alpha-database", SkillSource::Project, "database helper"),
        ]);

        let output = run_discover_skills(&serde_json::json!({
            "description": "database",
            "limit": 10,
            "source": "all"
        }))
        .unwrap();

        assert_eq!(output["query"], "database");
        assert_eq!(output["skills"][0]["name"], "alpha-database");
        assert_eq!(output["skills"][1]["name"], "beta-database");
    }

    #[test]
    #[serial]
    fn mcp_skill_results_include_provenance_for_mcp_search_summaries() {
        let _guard = SkillRegistryGuard::new(vec![make_skill(
            "linear-review",
            SkillSource::Mcp("linear".to_string()),
            "Review Linear issue context",
        )]);

        let results = mcp_skill_summary_results("linear", 10).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, DiscoveryResultKind::McpSkill);
        assert_eq!(results[0].server_name.as_deref(), Some("linear"));
        assert_eq!(
            results[0].skill_summaries[0].source.as_deref(),
            Some("mcp:linear")
        );
    }

    #[test]
    #[serial]
    fn skill_search_prefetch_metadata_absent_when_experimental_feature_disabled() {
        let mut flags = FeatureFlags::all_enabled();
        flags.experimental_skill_search = false;
        allthecodes_config::features::set_runtime_override(flags);
        let _feature_guard = FeatureOverrideGuard;
        let _skills = SkillRegistryGuard::new(vec![make_skill(
            "rust-review",
            SkillSource::User,
            "Review Rust code",
        )]);

        let output = run_skill_search(DiscoverySearchInput {
            query: "rust".to_string(),
            source_filter: Some("all".to_string()),
            max_results: 10,
            include_summaries: true,
        })
        .unwrap();

        assert!(output.data["results"][0].get("prefetch_source").is_none());
        assert_eq!(output.data["results"][0]["remote_url_todo"], false);
    }
}
