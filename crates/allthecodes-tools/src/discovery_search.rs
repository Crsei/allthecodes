use std::collections::HashSet;
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, LazyLock};

use anyhow::Result;
use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::common::{string_param, validate_enum};
use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, Tools, ValidationResult};
use allthecodes_config::features::{self, Feature};
use allthecodes_types::message::AssistantMessage;

pub type DiscoverySearchProvider =
    Arc<dyn Fn() -> Vec<DiscoverySearchResult> + Send + Sync + 'static>;

#[derive(Clone, Default)]
pub struct DiscoverySearchRuntime {
    mcp_items_provider: Option<DiscoverySearchProvider>,
    plugin_items_provider: Option<DiscoverySearchProvider>,
}

impl DiscoverySearchRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_mcp_items_provider<F>(mut self, provider: F) -> Self
    where
        F: Fn() -> Vec<DiscoverySearchResult> + Send + Sync + 'static,
    {
        self.mcp_items_provider = Some(Arc::new(provider));
        self
    }

    pub fn with_plugin_items_provider<F>(mut self, provider: F) -> Self
    where
        F: Fn() -> Vec<DiscoverySearchResult> + Send + Sync + 'static,
    {
        self.plugin_items_provider = Some(Arc::new(provider));
        self
    }
}

static DISCOVERY_SEARCH_RUNTIME: LazyLock<RwLock<DiscoverySearchRuntime>> =
    LazyLock::new(|| RwLock::new(DiscoverySearchRuntime::new()));

pub fn install_discovery_search_runtime(runtime: DiscoverySearchRuntime) {
    *DISCOVERY_SEARCH_RUNTIME.write() = runtime;
}

pub fn installed_discovery_search_runtime() -> DiscoverySearchRuntime {
    DISCOVERY_SEARCH_RUNTIME.read().clone()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverySearchError {
    message: String,
}

impl DiscoverySearchError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for DiscoverySearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DiscoverySearchError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryResultKind {
    Skill,
    McpServer,
    McpResource,
    McpCapability,
    McpSkill,
    Plugin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverySignal {
    ExplicitSearch,
    PrefetchSearch,
    McpResourceDiscovery,
    PluginMarketplaceCache,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryStatusSummary {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl DiscoveryStatusSummary {
    pub fn new(state: impl Into<String>) -> Self {
        Self {
            state: state.into(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryNextAction {
    pub label: String,
    pub command: String,
}

impl DiscoveryNextAction {
    pub fn new(label: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            command: command.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryToolSummary {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl DiscoveryToolSummary {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: Some(description.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverySkillSummary {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl DiscoverySkillSummary {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: Some(description.into()),
            source: None,
        }
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillPrefetchResult {
    pub query: String,
    pub results: Vec<DiscoverySearchResult>,
    pub remote_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverySearchResult {
    pub kind: DiscoveryResultKind,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when_to_use: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_invocable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_invocable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_summary: Option<DiscoveryStatusSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_summaries: Vec<DiscoveryToolSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skill_summaries: Vec<DiscoverySkillSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marketplace: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub match_reasons: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<DiscoveryNextAction>,
    pub discovery_signal: DiscoverySignal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefetch_source: Option<String>,
    pub remote_url_todo: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_source: Option<String>,
}

impl DiscoverySearchResult {
    pub fn new(kind: DiscoveryResultKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            id: None,
            display_name: None,
            source: None,
            server_name: None,
            version: None,
            description: None,
            when_to_use: None,
            user_invocable: None,
            model_invocable: None,
            status_summary: None,
            capabilities: Vec::new(),
            tool_summaries: Vec::new(),
            skill_summaries: Vec::new(),
            skills: Vec::new(),
            tools: Vec::new(),
            mcp_servers: Vec::new(),
            marketplace: None,
            match_reasons: Vec::new(),
            next_action: None,
            discovery_signal: DiscoverySignal::ExplicitSearch,
            prefetch_source: None,
            remote_url_todo: false,
            remote_source: None,
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn with_display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name = Some(display_name.into());
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn with_server_name(mut self, server_name: impl Into<String>) -> Self {
        self.server_name = Some(server_name.into());
        self
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_when_to_use(mut self, when_to_use: impl Into<String>) -> Self {
        self.when_to_use = Some(when_to_use.into());
        self
    }

    pub fn with_invocation_flags(mut self, user_invocable: bool, model_invocable: bool) -> Self {
        self.user_invocable = Some(user_invocable);
        self.model_invocable = Some(model_invocable);
        self
    }

    pub fn with_status(mut self, status: DiscoveryStatusSummary) -> Self {
        self.status_summary = Some(status);
        self
    }

    pub fn with_capabilities<I, S>(mut self, capabilities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.capabilities = capabilities.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_tool_summaries<I>(mut self, summaries: I) -> Self
    where
        I: IntoIterator<Item = DiscoveryToolSummary>,
    {
        self.tool_summaries = summaries.into_iter().collect();
        self
    }

    pub fn with_skill_summaries<I>(mut self, summaries: I) -> Self
    where
        I: IntoIterator<Item = DiscoverySkillSummary>,
    {
        self.skill_summaries = summaries.into_iter().collect();
        self
    }

    pub fn with_skills<I, S>(mut self, skills: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.skills = skills.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_tools<I, S>(mut self, tools: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tools = tools.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_mcp_servers<I, S>(mut self, servers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.mcp_servers = servers.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_marketplace(mut self, marketplace: impl Into<String>) -> Self {
        self.marketplace = Some(marketplace.into());
        self
    }

    pub fn with_next_action(mut self, action: DiscoveryNextAction) -> Self {
        self.next_action = Some(action);
        self
    }

    pub fn with_signal(mut self, signal: DiscoverySignal) -> Self {
        self.discovery_signal = signal;
        self
    }

    pub fn with_prefetch_source(mut self, source: impl Into<String>) -> Self {
        self.prefetch_source = Some(source.into());
        self
    }

    pub fn with_remote_state_placeholder(mut self) -> Self {
        if features::enabled(Feature::RemoteUrlDiscovery) {
            self.remote_url_todo = true;
            self.remote_source = Some("deferred".to_string());
        } else {
            self.remote_url_todo = false;
            self.remote_source = Some("feature_disabled".to_string());
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverySearchInput {
    pub query: String,
    pub source_filter: Option<String>,
    pub max_results: usize,
    pub include_summaries: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverySearchOutput {
    pub data: Value,
    pub display_preview: Option<String>,
}

#[derive(Debug, Clone)]
struct ProviderItems {
    installed: bool,
    items: Vec<DiscoverySearchResult>,
    error: Option<String>,
}

pub fn tools() -> Tools {
    vec![Arc::new(McpSearchTool), Arc::new(PluginSearchTool)]
}

pub fn search_items(
    query: &str,
    max_results: usize,
    source_filter: Option<&str>,
) -> std::result::Result<Vec<DiscoverySearchResult>, DiscoverySearchError> {
    let runtime = installed_discovery_search_runtime();
    let mut items = provider_items(runtime.mcp_items_provider.as_ref()).items;
    items.extend(provider_items(runtime.plugin_items_provider.as_ref()).items);
    search_items_in(query, max_results, source_filter, items)
}

pub fn search_items_in(
    query: &str,
    max_results: usize,
    source_filter: Option<&str>,
    items: Vec<DiscoverySearchResult>,
) -> std::result::Result<Vec<DiscoverySearchResult>, DiscoverySearchError> {
    validate_query(query)?;
    let source_filter = normalized_filter(source_filter);
    let mut matches = items
        .into_iter()
        .filter(|item| source_matches(item, source_filter))
        .filter_map(|mut item| {
            let (score, reasons) = score_item(query, &item);
            if score == 0 {
                return None;
            }
            item.match_reasons = reasons;
            Some((score, item))
        })
        .collect::<Vec<_>>();

    matches.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.name.cmp(&b.1.name))
            .then_with(|| a.1.id.cmp(&b.1.id))
    });
    matches.truncate(max_results.clamp(1, 100));
    Ok(matches.into_iter().map(|(_, item)| item).collect())
}

pub fn run_mcp_search(
    input: DiscoverySearchInput,
) -> std::result::Result<DiscoverySearchOutput, DiscoverySearchError> {
    validate_query(&input.query)?;
    let runtime = installed_discovery_search_runtime();
    let provider = provider_items(runtime.mcp_items_provider.as_ref());
    run_provider_search(
        input,
        provider,
        "scope",
        "MCP discovery provider is not installed",
        "MCP discovery provider failed",
        DomainOutputOptions {
            include_summaries_key: "include_tool_summaries",
            redact_contributions_when_false: false,
        },
    )
}

pub fn run_plugin_search(
    input: DiscoverySearchInput,
) -> std::result::Result<DiscoverySearchOutput, DiscoverySearchError> {
    validate_query(&input.query)?;
    let runtime = installed_discovery_search_runtime();
    let provider = provider_items(runtime.plugin_items_provider.as_ref());
    run_provider_search(
        input,
        provider,
        "source",
        "Plugin discovery provider is not installed",
        "Plugin discovery provider failed",
        DomainOutputOptions {
            include_summaries_key: "include_contributions",
            redact_contributions_when_false: true,
        },
    )
}

fn validate_query(query: &str) -> std::result::Result<(), DiscoverySearchError> {
    if query.trim().is_empty() {
        return Err(DiscoverySearchError::new("query is required"));
    }
    Ok(())
}

fn normalized_filter(filter: Option<&str>) -> Option<&str> {
    filter
        .map(str::trim)
        .filter(|filter| !filter.is_empty() && *filter != "all")
}

fn source_matches(item: &DiscoverySearchResult, filter: Option<&str>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    item.source.as_deref() == Some(filter)
}

fn provider_items(provider: Option<&DiscoverySearchProvider>) -> ProviderItems {
    let Some(provider) = provider else {
        return ProviderItems {
            installed: false,
            items: Vec::new(),
            error: None,
        };
    };

    match catch_unwind(AssertUnwindSafe(|| provider())) {
        Ok(items) => ProviderItems {
            installed: true,
            items,
            error: None,
        },
        Err(_) => ProviderItems {
            installed: true,
            items: Vec::new(),
            error: Some("provider panicked while collecting discovery data".to_string()),
        },
    }
}

struct DomainOutputOptions {
    include_summaries_key: &'static str,
    redact_contributions_when_false: bool,
}

fn run_provider_search(
    input: DiscoverySearchInput,
    provider: ProviderItems,
    filter_key: &str,
    missing_message: &str,
    failed_message: &str,
    options: DomainOutputOptions,
) -> std::result::Result<DiscoverySearchOutput, DiscoverySearchError> {
    let filter = input.source_filter.as_deref().unwrap_or("all");
    if !provider.installed {
        let data = json!({
            "query": input.query,
            filter_key: filter,
            "count": 0,
            "results": [],
            "note": missing_message,
        });
        return Ok(DiscoverySearchOutput {
            data,
            display_preview: Some(format!("{missing_message}; returning empty local results.")),
        });
    }
    if let Some(error) = provider.error {
        let data = json!({
            "query": input.query,
            filter_key: filter,
            "count": 0,
            "results": [],
            "note": failed_message,
            "error": error,
        });
        return Ok(DiscoverySearchOutput {
            data,
            display_preview: Some(format!("{failed_message}; returning empty local results.")),
        });
    }

    let mut results = search_items_in(
        &input.query,
        input.max_results,
        input.source_filter.as_deref(),
        provider.items,
    )?;
    if !input.include_summaries {
        for result in &mut results {
            result.tool_summaries.clear();
            result.skill_summaries.clear();
            if options.redact_contributions_when_false {
                result.skills.clear();
                result.tools.clear();
                result.mcp_servers.clear();
            }
        }
    }
    let data = json!({
        "query": input.query,
        filter_key: filter,
        "count": results.len(),
        "results": results,
        options.include_summaries_key: input.include_summaries,
    });
    Ok(DiscoverySearchOutput {
        data,
        display_preview: None,
    })
}

fn score_item(query: &str, item: &DiscoverySearchResult) -> (usize, Vec<String>) {
    let fields = item_fields(item);
    let score = text_score(
        query,
        &fields
            .iter()
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>(),
    );
    if score == 0 {
        return (0, Vec::new());
    }

    let mut reasons = Vec::new();
    for (reason, value) in fields {
        if field_matches_query(&value, query) && !reasons.contains(&reason) {
            reasons.push(reason);
        }
    }
    if reasons.is_empty() {
        reasons.push("metadata".to_string());
    }
    (score, reasons)
}

fn item_fields(item: &DiscoverySearchResult) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    fields.push(("name".to_string(), item.name.clone()));
    push_opt(&mut fields, "id", &item.id);
    push_opt(&mut fields, "display_name", &item.display_name);
    push_opt(&mut fields, "source", &item.source);
    push_opt(&mut fields, "server_name", &item.server_name);
    push_opt(&mut fields, "description", &item.description);
    push_opt(&mut fields, "when_to_use", &item.when_to_use);
    push_opt(&mut fields, "version", &item.version);
    push_opt(&mut fields, "marketplace", &item.marketplace);
    if let Some(status) = &item.status_summary {
        fields.push(("status".to_string(), status.state.clone()));
        push_opt(&mut fields, "status", &status.detail);
    }
    push_joined(&mut fields, "capability", &item.capabilities);
    push_joined(&mut fields, "tool", &item.tools);
    push_joined(&mut fields, "skill", &item.skills);
    push_joined(&mut fields, "mcp_server", &item.mcp_servers);
    for summary in &item.tool_summaries {
        fields.push(("tool".to_string(), summary.name.clone()));
        push_opt(&mut fields, "tool", &summary.description);
    }
    for summary in &item.skill_summaries {
        fields.push(("skill".to_string(), summary.name.clone()));
        push_opt(&mut fields, "skill", &summary.description);
        push_opt(&mut fields, "skill", &summary.source);
    }
    fields
}

fn push_opt(fields: &mut Vec<(String, String)>, reason: &str, value: &Option<String>) {
    if let Some(value) = value.as_ref().filter(|value| !value.trim().is_empty()) {
        fields.push((reason.to_string(), value.clone()));
    }
}

fn push_joined(fields: &mut Vec<(String, String)>, reason: &str, values: &[String]) {
    if !values.is_empty() {
        fields.push((reason.to_string(), values.join(" ")));
    }
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

fn text_score(query: &str, fields: &[&str]) -> usize {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return 0;
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

pub struct McpSearchTool;
pub struct PluginSearchTool;

fn domain_query(input: &Value) -> &str {
    string_param(input, "query").unwrap_or("")
}

fn domain_max_results(input: &Value) -> usize {
    input
        .get("max_results")
        .and_then(Value::as_u64)
        .unwrap_or(25)
        .clamp(1, 100) as usize
}

fn validation_error(message: impl Into<String>) -> ValidationResult {
    ValidationResult::Error {
        message: message.into(),
        error_code: 400,
    }
}

fn validate_required_query(input: &Value) -> Option<ValidationResult> {
    if domain_query(input).trim().is_empty() {
        return Some(validation_error("query is required"));
    }
    None
}

#[async_trait]
impl Tool for McpSearchTool {
    fn name(&self) -> &str {
        "McpSearch"
    }

    async fn description(&self, _input: &Value) -> String {
        "Discover MCP servers, resources, capabilities, and MCP-provided skills without returning callable tool schemas.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Text query over MCP server names, resources, capabilities, tools, and MCP skills."},
                "scope": {"type": "string", "enum": ["all", "user", "project", "runtime"], "description": "MCP discovery scope filter."},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 100},
                "include_tool_summaries": {"type": "boolean", "description": "Include MCP tool names and descriptions without schemas."}
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
        if let Some(result) = validate_required_query(input) {
            return result;
        }
        if let Some(result) = validate_enum(input, "scope", &["all", "user", "project", "runtime"])
        {
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
        let output = run_mcp_search(DiscoverySearchInput {
            query: domain_query(&input).to_string(),
            source_filter: string_param(&input, "scope").map(ToOwned::to_owned),
            max_results: domain_max_results(&input),
            include_summaries: input
                .get("include_tool_summaries")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        })?;
        Ok(ToolResult {
            data: output.data,
            display_preview: output.display_preview,
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Discover MCP servers, resources, capabilities, and MCP-provided skills. Use ToolSearch for exact MCP callable tool schemas.".into()
    }
}

#[async_trait]
impl Tool for PluginSearchTool {
    fn name(&self) -> &str {
        "PluginSearch"
    }

    async fn description(&self, _input: &Value) -> String {
        "Discover installed, active, and marketplace-cache plugins without returning callable tool schemas.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Text query over plugin names, descriptions, status, skills, tools, and MCP contributions."},
                "source": {"type": "string", "enum": ["all", "installed", "active", "marketplace_cache"], "description": "Plugin source filter."},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 100},
                "include_contributions": {"type": "boolean", "description": "Include plugin skill/tool/MCP contribution summaries without schemas."}
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
        if let Some(result) = validate_required_query(input) {
            return result;
        }
        if let Some(result) = validate_enum(
            input,
            "source",
            &["all", "installed", "active", "marketplace_cache"],
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
        let output = run_plugin_search(DiscoverySearchInput {
            query: domain_query(&input).to_string(),
            source_filter: string_param(&input, "source").map(ToOwned::to_owned),
            max_results: domain_max_results(&input),
            include_summaries: input
                .get("include_contributions")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        })?;
        Ok(ToolResult {
            data: output.data,
            display_preview: output.display_preview,
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Discover plugin capabilities and status summaries. Use ToolSearch for exact plugin callable tool schemas.".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_config::features::{self, FeatureFlags};
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

    fn rust_plugin() -> DiscoverySearchResult {
        DiscoverySearchResult::new(DiscoveryResultKind::Plugin, "rust-tools")
            .with_id("rust-tools")
            .with_source("active")
            .with_description("Rust formatting, linting, and workspace analysis")
            .with_status(DiscoveryStatusSummary::new("installed"))
            .with_tools(["plugin__rust_tools__fmt"])
            .with_skills(["rust-review"])
            .with_next_action(DiscoveryNextAction::new(
                "Inspect plugin",
                "/plugin info rust-tools",
            ))
            .with_signal(DiscoverySignal::PluginMarketplaceCache)
            .with_remote_state_placeholder()
    }

    fn github_mcp() -> DiscoverySearchResult {
        DiscoverySearchResult::new(DiscoveryResultKind::McpCapability, "github pull requests")
            .with_server_name("github")
            .with_source("runtime")
            .with_description("Create and review pull requests through the github MCP server")
            .with_status(DiscoveryStatusSummary::new("connected"))
            .with_capabilities(["tools", "resources"])
            .with_tool_summaries([DiscoveryToolSummary::new(
                "mcp__github__create_pull_request",
                "Create a pull request",
            )])
            .with_next_action(DiscoveryNextAction::new(
                "Inspect callable schema",
                "ToolSearch source=mcp query=select:mcp__github__create_pull_request",
            ))
            .with_signal(DiscoverySignal::McpResourceDiscovery)
    }

    #[test]
    #[serial]
    fn search_items_ranks_filters_and_clamps_provider_data() {
        install_discovery_search_runtime(
            DiscoverySearchRuntime::new()
                .with_mcp_items_provider(|| vec![github_mcp()])
                .with_plugin_items_provider(|| {
                    vec![
                        rust_plugin(),
                        DiscoverySearchResult::new(DiscoveryResultKind::Plugin, "node-tools")
                            .with_id("node-tools")
                            .with_source("installed")
                            .with_description("JavaScript package helpers"),
                    ]
                }),
        );

        let results = search_items("rust formatter", 1, Some("active")).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id.as_deref(), Some("rust-tools"));
        assert_eq!(results[0].source.as_deref(), Some("active"));
        assert_eq!(
            results[0].discovery_signal,
            DiscoverySignal::PluginMarketplaceCache
        );
        assert_eq!(results[0].match_reasons[0], "name");
    }

    #[test]
    #[serial]
    fn search_items_rejects_empty_query() {
        install_discovery_search_runtime(DiscoverySearchRuntime::new());

        let err = search_items("   ", 10, None).unwrap_err();

        assert!(err.to_string().contains("query is required"));
    }

    #[test]
    #[serial]
    fn mcp_search_without_provider_returns_empty_preview() {
        install_discovery_search_runtime(DiscoverySearchRuntime::new());

        let output = run_mcp_search(DiscoverySearchInput {
            query: "github".to_string(),
            source_filter: Some("runtime".to_string()),
            max_results: 10,
            include_summaries: true,
        })
        .unwrap();

        assert_eq!(output.data["count"], 0);
        assert!(output.data["results"].as_array().unwrap().is_empty());
        assert!(output
            .display_preview
            .as_deref()
            .unwrap()
            .contains("MCP discovery provider is not installed"));
    }

    #[test]
    #[serial]
    fn plugin_search_remote_state_fields_are_present_but_inert() {
        let mut flags = FeatureFlags::all_disabled();
        flags.remote_url_discovery = true;
        let _features = FeatureGuard::set(flags);
        install_discovery_search_runtime(
            DiscoverySearchRuntime::new().with_plugin_items_provider(|| vec![rust_plugin()]),
        );

        let output = run_plugin_search(DiscoverySearchInput {
            query: "rust".to_string(),
            source_filter: Some("all".to_string()),
            max_results: 5,
            include_summaries: true,
        })
        .unwrap();

        let result = &output.data["results"][0];
        assert_eq!(result["remote_url_todo"], true);
        assert_eq!(result["remote_source"], "deferred");
        assert!(result.get("remote_url").is_none());
    }

    #[test]
    #[serial]
    fn plugin_search_marks_remote_url_discovery_feature_disabled_when_gate_is_off() {
        let _features = FeatureGuard::set(FeatureFlags::all_disabled());
        install_discovery_search_runtime(
            DiscoverySearchRuntime::new().with_plugin_items_provider(|| vec![rust_plugin()]),
        );

        let output = run_plugin_search(DiscoverySearchInput {
            query: "rust".to_string(),
            source_filter: Some("all".to_string()),
            max_results: 5,
            include_summaries: true,
        })
        .unwrap();

        let result = &output.data["results"][0];
        assert_eq!(result["remote_url_todo"], false);
        assert_eq!(result["remote_source"], "feature_disabled");
        assert!(result.get("remote_url").is_none());
    }

    #[test]
    fn registry_exposes_domain_search_tools_without_losing_tool_search() {
        let names = crate::registry::get_all_tools()
            .into_iter()
            .map(|tool| tool.name().to_string())
            .collect::<Vec<_>>();

        for expected in ["ToolSearch", "SkillSearch", "McpSearch", "PluginSearch"] {
            assert!(
                names.contains(&expected.to_string()),
                "{expected} should be registered"
            );
        }
    }
}
