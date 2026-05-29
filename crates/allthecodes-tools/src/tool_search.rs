//! ToolSearch tool and ranking index.
//!
//! The index keeps the cheap retrieval surface (name, aliases, category,
//! description) separate from input schema hydration. Schema terms are loaded
//! only when a query is schema-shaped or when the caller explicitly asks for a
//! selected result's schema.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, LazyLock, OnceLock};

use anyhow::Result;
use async_trait::async_trait;
use parking_lot::RwLock;
use serde::Serialize;
use serde_json::{json, Value};

use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, Tools, ValidationResult};
use allthecodes_types::message::AssistantMessage;

const DEFAULT_LIMIT: usize = 8;
const MAX_LIMIT: usize = 50;
const MIN_SCORE: f64 = 0.01;

static RUNTIME_TOOLS: LazyLock<RwLock<Tools>> = LazyLock::new(|| RwLock::new(Vec::new()));

type SchemaLoader = Arc<dyn Fn() -> Value + Send + Sync>;

/// Install the session's fully merged tool list for future ToolSearch calls.
///
/// Startup calls this after plugin, MCP, Computer Use, and built-in tools have
/// been merged. Searches still merge in the current registry snapshot on every
/// call so plugin refreshes can be seen where the registry can discover them.
pub fn install_runtime_tool_catalog(tools: &[Arc<dyn Tool>]) {
    *RUNTIME_TOOLS.write() = tools.to_vec();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSearchSource {
    Builtin,
    Plugin,
    Mcp,
    Skill,
}

impl ToolSearchSource {
    fn priority(self) -> usize {
        match self {
            ToolSearchSource::Builtin => 0,
            ToolSearchSource::Plugin => 1,
            ToolSearchSource::Mcp => 2,
            ToolSearchSource::Skill => 3,
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "builtin" | "built-in" | "core" => Some(Self::Builtin),
            "plugin" | "plugins" => Some(Self::Plugin),
            "mcp" => Some(Self::Mcp),
            "skill" | "skills" => Some(Self::Skill),
            "all" | "" => None,
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolSearchOptions {
    pub limit: usize,
    pub source: Option<ToolSearchSource>,
    pub include_schema: bool,
}

impl Default for ToolSearchOptions {
    fn default() -> Self {
        Self {
            limit: DEFAULT_LIMIT,
            source: None,
            include_schema: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolSearchResult {
    pub name: String,
    pub display_name: String,
    pub source: ToolSearchSource,
    pub category: String,
    pub description: String,
    pub score: f64,
    pub matched_terms: Vec<String>,
    pub reasons: Vec<String>,
    pub schema_loaded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invocation: Option<Value>,
}

#[derive(Debug, Clone)]
struct NormalizedQuery {
    phrase: String,
    tokens: Vec<String>,
    is_select: bool,
}

#[derive(Clone)]
pub struct ToolSearchDocument {
    name: String,
    display_name: String,
    description: String,
    source: ToolSearchSource,
    category: String,
    aliases: Vec<String>,
    invocation: Option<Value>,
    ordinal: usize,
    name_tokens: Vec<String>,
    alias_tokens: Vec<String>,
    description_tokens: Vec<String>,
    tag_tokens: Vec<String>,
    vector: BTreeMap<String, f64>,
    schema_loader: Option<SchemaLoader>,
    schema_value: Arc<OnceLock<Value>>,
    schema_tokens: Arc<OnceLock<Vec<String>>>,
}

impl ToolSearchDocument {
    async fn from_tool(tool: Arc<dyn Tool>, ordinal: usize) -> Option<Self> {
        if !tool.is_enabled() {
            return None;
        }

        let empty_input = json!({});
        let name = tool.name().to_string();
        let display_name = tool.user_facing_name(None);
        let description = tool.description(&empty_input).await;
        let source = infer_tool_source(&name, &display_name, &description);
        let category = infer_tool_category(source, &name, tool.as_ref());
        let aliases = aliases_for_tool(&name, &display_name, source);
        let tags = tags_for_tool(source, &category, tool.as_ref());
        let schema_tool = tool.clone();

        Some(Self::new(DocumentInit {
            name: name.clone(),
            display_name,
            description,
            source,
            category,
            tags,
            aliases,
            invocation: Some(json!({ "tool": name })),
            ordinal,
            schema_loader: Some(Arc::new(move || schema_tool.input_json_schema())),
        }))
    }

    fn from_skill(skill: &allthecodes_skills::SkillDefinition, ordinal: usize) -> Self {
        let mut description = skill.frontmatter.description.clone();
        if let Some(when) = skill.frontmatter.when_to_use.as_deref() {
            if !when.trim().is_empty() {
                if !description.is_empty() {
                    description.push(' ');
                }
                description.push_str("Use when: ");
                description.push_str(when);
            }
        }

        let mut tags = vec!["skill".to_string(), format!("{:?}", skill.source)];
        tags.extend(skill.frontmatter.allowed_tools.iter().cloned());

        Self::new(DocumentInit {
            name: format!("skill:{}", skill.name),
            display_name: format!("Skill: {}", skill.display_name()),
            description,
            source: ToolSearchSource::Skill,
            category: "skill".to_string(),
            tags,
            aliases: vec![skill.name.clone(), skill.display_name().to_string()],
            invocation: Some(json!({
                "tool": "Skill",
                "input": { "skill": skill.name }
            })),
            ordinal,
            schema_loader: None,
        })
    }

    fn new(init: DocumentInit) -> Self {
        let name_tokens = tokenize_and_stem(&init.name);
        let display_tokens = tokenize_and_stem(&init.display_name);
        let alias_tokens = init
            .aliases
            .iter()
            .flat_map(|alias| tokenize_and_stem(alias))
            .collect::<Vec<_>>();
        let description_tokens = tokenize_and_stem(&init.description);
        let tag_tokens = init
            .tags
            .iter()
            .chain(std::iter::once(&init.category))
            .flat_map(|tag| tokenize_and_stem(tag))
            .collect::<Vec<_>>();

        let mut combined_name_tokens = name_tokens;
        combined_name_tokens.extend(display_tokens);

        let vector = weighted_tf(&[
            (&combined_name_tokens, 3.5),
            (&alias_tokens, 2.5),
            (&tag_tokens, 1.6),
            (&description_tokens, 1.0),
        ]);

        Self {
            name: init.name,
            display_name: init.display_name,
            description: init.description,
            source: init.source,
            category: init.category,
            aliases: init.aliases,
            invocation: init.invocation,
            ordinal: init.ordinal,
            name_tokens: combined_name_tokens,
            alias_tokens,
            description_tokens,
            tag_tokens,
            vector,
            schema_loader: init.schema_loader,
            schema_value: Arc::new(OnceLock::new()),
            schema_tokens: Arc::new(OnceLock::new()),
        }
    }

    fn hydrate_schema(&self) -> Option<&Value> {
        let loader = self.schema_loader.as_ref()?;
        Some(self.schema_value.get_or_init(|| loader()))
    }

    fn schema_tokens(&self) -> &[String] {
        self.schema_tokens
            .get_or_init(|| {
                self.hydrate_schema()
                    .map(extract_schema_terms)
                    .unwrap_or_default()
            })
            .as_slice()
    }

    fn identity_keys(&self) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        keys.insert(normalize_identity(&self.name));
        keys.insert(normalize_identity(&self.display_name));
        for alias in &self.aliases {
            keys.insert(normalize_identity(alias));
        }
        keys
    }
}

struct DocumentInit {
    name: String,
    display_name: String,
    description: String,
    source: ToolSearchSource,
    category: String,
    tags: Vec<String>,
    aliases: Vec<String>,
    invocation: Option<Value>,
    ordinal: usize,
    schema_loader: Option<SchemaLoader>,
}

pub struct ToolSearchIndex {
    docs: Vec<ToolSearchDocument>,
    idf: BTreeMap<String, f64>,
}

impl ToolSearchIndex {
    pub async fn from_tools(tools: Tools) -> Self {
        let mut docs = Vec::new();
        for (ordinal, tool) in tools.into_iter().enumerate() {
            if let Some(doc) = ToolSearchDocument::from_tool(tool, ordinal).await {
                docs.push(doc);
            }
        }
        Self::new(docs)
    }

    fn new(mut docs: Vec<ToolSearchDocument>) -> Self {
        for doc in &mut docs {
            doc.vector = base_vector(doc);
        }
        let idf = compute_idf(&docs);
        for doc in &mut docs {
            for (term, weight) in doc.vector.iter_mut() {
                *weight *= idf.get(term).copied().unwrap_or(1.0);
            }
        }
        Self { docs, idf }
    }

    fn with_skills(mut self, skills: Vec<allthecodes_skills::SkillDefinition>) -> Self {
        let start = self.docs.len();
        for (offset, skill) in skills
            .into_iter()
            .filter(|s| s.is_model_invocable())
            .enumerate()
        {
            self.docs
                .push(ToolSearchDocument::from_skill(&skill, start + offset));
        }
        Self::new(self.docs)
    }

    pub fn search(&self, query: &str, options: ToolSearchOptions) -> Vec<ToolSearchResult> {
        let normalized = normalize_query(query);
        if normalized.tokens.is_empty() {
            return Vec::new();
        }

        if normalized.is_select {
            return self.select_exact(&normalized, options);
        }

        let query_vector = query_vector(&normalized.tokens, &self.idf);
        let base_max = self
            .docs
            .iter()
            .filter(|doc| source_allowed(doc.source, options.source))
            .map(|doc| self.score_doc(doc, &normalized, &query_vector, false))
            .map(|score| score.score)
            .fold(0.0, f64::max);
        let use_schema = options.include_schema
            || (base_max < 80.0 && normalized.tokens.iter().any(|t| is_schema_probe_term(t)));

        let mut scored = self
            .docs
            .iter()
            .filter(|doc| source_allowed(doc.source, options.source))
            .map(|doc| self.score_doc(doc, &normalized, &query_vector, use_schema))
            .filter(|score| score.score >= MIN_SCORE)
            .collect::<Vec<_>>();

        sort_scored(&mut scored);
        scored
            .into_iter()
            .take(options.limit.min(MAX_LIMIT))
            .map(|score| score.into_result(options.include_schema))
            .collect()
    }

    fn select_exact(
        &self,
        query: &NormalizedQuery,
        options: ToolSearchOptions,
    ) -> Vec<ToolSearchResult> {
        let wanted = normalize_identity(&query.phrase);
        let mut matches = self
            .docs
            .iter()
            .filter(|doc| source_allowed(doc.source, options.source))
            .filter(|doc| doc.identity_keys().contains(&wanted))
            .map(|doc| ScoredDocument {
                doc,
                score: 1000.0,
                matched_terms: query.tokens.to_vec(),
                reasons: vec!["exact select match".to_string()],
                schema_loaded_for_ranking: true,
            })
            .collect::<Vec<_>>();
        sort_scored(&mut matches);
        matches
            .into_iter()
            .take(options.limit.min(MAX_LIMIT))
            .map(|score| score.into_result(true))
            .collect()
    }

    fn score_doc<'a>(
        &'a self,
        doc: &'a ToolSearchDocument,
        query: &NormalizedQuery,
        query_vector: &BTreeMap<String, f64>,
        use_schema: bool,
    ) -> ScoredDocument<'a> {
        let mut score = 0.0;
        let mut matched = BTreeSet::new();
        let mut reasons = Vec::new();

        let doc_name = normalize_identity(&doc.name);
        let display_name = normalize_identity(&doc.display_name);
        let query_identity = normalize_identity(&query.phrase);

        if doc_name == query_identity || display_name == query_identity {
            score += 120.0;
            reasons.push("exact name match".to_string());
        } else if doc_name.starts_with(&query_identity) || display_name.starts_with(&query_identity)
        {
            score += 70.0;
            reasons.push("name prefix match".to_string());
        } else if doc_name.contains(&query_identity) || display_name.contains(&query_identity) {
            score += 45.0;
            reasons.push("name substring match".to_string());
        }

        for alias in &doc.aliases {
            let alias_key = normalize_identity(alias);
            if alias_key == query_identity {
                score += 65.0;
                reasons.push("exact alias match".to_string());
            } else if alias_key.starts_with(&query_identity) {
                score += 35.0;
                reasons.push("alias prefix match".to_string());
            }
        }

        let cosine = cosine_similarity(query_vector, &doc.vector);
        if cosine > 0.0 {
            score += cosine * 45.0;
            reasons.push("weighted text match".to_string());
        }

        score += overlap_score(&query.tokens, &doc.name_tokens, 11.0, &mut matched);
        score += overlap_score(&query.tokens, &doc.alias_tokens, 8.0, &mut matched);
        score += overlap_score(&query.tokens, &doc.tag_tokens, 5.0, &mut matched);
        score += overlap_score(&query.tokens, &doc.description_tokens, 3.5, &mut matched);

        let mut schema_loaded_for_ranking = false;
        if use_schema {
            let schema_tokens = doc.schema_tokens();
            let before = matched.len();
            score += overlap_score(&query.tokens, schema_tokens, 2.75, &mut matched);
            if matched.len() > before {
                reasons.push("input schema match".to_string());
            }
            schema_loaded_for_ranking = doc.schema_loader.is_some();
        }

        if query.tokens.iter().all(|t| matched.contains(t)) && !matched.is_empty() {
            score += 5.0;
            reasons.push("all query terms matched".to_string());
        }

        ScoredDocument {
            doc,
            score,
            matched_terms: matched.into_iter().collect(),
            reasons: dedupe_reasons(reasons),
            schema_loaded_for_ranking,
        }
    }
}

struct ScoredDocument<'a> {
    doc: &'a ToolSearchDocument,
    score: f64,
    matched_terms: Vec<String>,
    reasons: Vec<String>,
    schema_loaded_for_ranking: bool,
}

impl ScoredDocument<'_> {
    fn into_result(self, include_schema: bool) -> ToolSearchResult {
        let input_schema = if include_schema {
            self.doc.hydrate_schema().cloned()
        } else {
            None
        };
        let schema_loaded = self.schema_loaded_for_ranking || input_schema.is_some();
        ToolSearchResult {
            name: self.doc.name.clone(),
            display_name: self.doc.display_name.clone(),
            source: self.doc.source,
            category: self.doc.category.clone(),
            description: self.doc.description.clone(),
            score: round_score(self.score),
            matched_terms: self.matched_terms,
            reasons: self.reasons,
            schema_loaded,
            input_schema,
            invocation: self.doc.invocation.clone(),
        }
    }
}

pub struct ToolSearchTool;

#[derive(Debug, serde::Deserialize)]
struct ToolSearchInput {
    query: String,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    include_schema: bool,
}

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "ToolSearch"
    }

    async fn description(&self, _input: &Value) -> String {
        "Search available tools and skills with deterministic ranking.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Natural-language query, tool name, or select:<tool-name> for an exact tool lookup."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_LIMIT,
                    "default": DEFAULT_LIMIT,
                    "description": "Maximum number of ranked results to return."
                },
                "source": {
                    "type": "string",
                    "enum": ["all", "builtin", "plugin", "mcp", "skill"],
                    "default": "all",
                    "description": "Optional source filter."
                },
                "include_schema": {
                    "type": "boolean",
                    "default": false,
                    "description": "When true, include hydrated input_schema for returned tool results."
                }
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let Some(query) = input.get("query").and_then(|v| v.as_str()) else {
            return ValidationResult::Error {
                message: "query is required".to_string(),
                error_code: 400,
            };
        };
        if query.trim().is_empty() {
            return ValidationResult::Error {
                message: "query cannot be empty".to_string(),
                error_code: 400,
            };
        }
        if let Some(source) = input.get("source").and_then(|v| v.as_str()) {
            if ToolSearchSource::parse(source).is_none()
                && !source.trim().eq_ignore_ascii_case("all")
                && !source.trim().is_empty()
            {
                return ValidationResult::Error {
                    message: format!("invalid source '{}'", source),
                    error_code: 400,
                };
            }
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let params: ToolSearchInput = serde_json::from_value(input)?;
        let index = build_runtime_index().await;
        let source = params.source.as_deref().and_then(ToolSearchSource::parse);
        let options = ToolSearchOptions {
            limit: params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
            source,
            include_schema: params.include_schema,
        };
        let normalized = normalize_query(&params.query);
        let results = index.search(&params.query, options);
        let preview = if results.is_empty() {
            format!("No tools found for '{}'.", params.query)
        } else {
            format!(
                "Found {} tool(s): {}",
                results.len(),
                results
                    .iter()
                    .take(5)
                    .map(|r| r.display_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        Ok(ToolResult {
            data: json!({
                "query": params.query,
                "normalized_query": {
                    "phrase": normalized.phrase,
                    "tokens": normalized.tokens,
                    "select": normalized.is_select,
                },
                "results": results,
            }),
            display_preview: Some(preview),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Use ToolSearch to discover available built-in, plugin, MCP, and skill tools. \
         Search by task intent, tool name, category, or input parameter. Use select:<tool-name> \
         with include_schema=true when you need a specific tool's full input schema."
            .to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "ToolSearch".to_string()
    }
}

async fn build_runtime_index() -> ToolSearchIndex {
    let tools = RUNTIME_TOOLS.read().clone();

    ToolSearchIndex::from_tools(tools)
        .await
        .with_skills(allthecodes_skills::get_model_invocable_skills())
}

fn source_allowed(source: ToolSearchSource, filter: Option<ToolSearchSource>) -> bool {
    filter.map(|f| f == source).unwrap_or(true)
}

fn infer_tool_source(name: &str, display_name: &str, description: &str) -> ToolSearchSource {
    if name.starts_with("mcp__") || display_name.starts_with("mcp__") {
        return ToolSearchSource::Mcp;
    }
    let lower_desc = description.to_ascii_lowercase();
    if lower_desc.contains("plugin tool") || lower_desc.contains("contributed by plugin") {
        return ToolSearchSource::Plugin;
    }
    ToolSearchSource::Builtin
}

fn infer_tool_category(source: ToolSearchSource, name: &str, tool: &dyn Tool) -> String {
    if source == ToolSearchSource::Mcp {
        return "mcp".to_string();
    }
    if source == ToolSearchSource::Plugin {
        return "plugin".to_string();
    }

    match name {
        "Read" | "Glob" | "Grep" | "WebFetch" | "WebSearch" => "read_only",
        "Edit" | "Write" | "NotebookEdit" => "edit",
        "Bash" | "PowerShell" | "Repl" | "Sleep" => "execution",
        "EnterPlanMode" | "ExitPlanMode" => "planning",
        "TodoWrite" | "TaskCreate" | "TaskGet" | "TaskUpdate" | "TaskList" | "TaskStop"
        | "TaskOutput" => "tasks",
        "Agent" | "Task" | "TeamSpawn" | "SendMessage" => "agent",
        "LSP" => "lsp",
        "SystemStatus" | "Config" | "StructuredOutput" | "AskUserQuestion" => "system",
        "SearchExtraTools" | "ExecuteExtraTool" => "tool_discovery",
        "Skill" => "skill",
        _ if tool.is_read_only(&json!({})) => "read_only",
        _ if tool.is_destructive(&json!({})) => "edit",
        _ => "other",
    }
    .to_string()
}

fn tags_for_tool(source: ToolSearchSource, category: &str, tool: &dyn Tool) -> Vec<String> {
    let mut tags = vec![format!("{:?}", source), category.to_string()];
    let empty = json!({});
    if tool.is_read_only(&empty) {
        tags.push("read_only".to_string());
    }
    if tool.is_concurrency_safe(&empty) {
        tags.push("concurrency_safe".to_string());
    }
    if tool.is_destructive(&empty) {
        tags.push("destructive".to_string());
    }
    tags
}

fn aliases_for_tool(name: &str, display_name: &str, source: ToolSearchSource) -> Vec<String> {
    let mut aliases = vec![display_name.to_string()];
    aliases.extend(
        (match name {
            "Bash" => vec!["shell", "command", "terminal", "execute", "run"],
            "PowerShell" => vec!["pwsh", "windows shell", "command", "terminal"],
            "Repl" => vec!["node", "python", "javascript", "execute code"],
            "Read" => vec!["open file", "view file", "cat", "file contents"],
            "Write" => vec!["create file", "save file", "overwrite file"],
            "Edit" => vec!["patch", "replace", "modify file", "update file"],
            "Glob" => vec!["find files", "filename pattern", "list files"],
            "Grep" => vec!["search files", "regex", "content search"],
            "CronCreate" => vec![
                "schedule task",
                "cron job",
                "recurring prompt",
                "automation",
            ],
            "CronDelete" => vec!["delete schedule", "remove cron", "cancel recurring task"],
            "CronList" => vec!["list schedules", "cron jobs", "scheduled tasks"],
            "WebBrowser" => vec!["javascript page", "rendered page", "browser", "web app"],
            "Monitor" => vec!["monitor command", "watch process", "background status"],
            "SendUserFile" => vec!["send file", "file attachment", "share file contents"],
            "CtxInspect" => vec!["context usage", "token budget", "message count"],
            "TerminalCapture" => vec!["terminal output", "shell output", "capture logs"],
            "ReviewArtifact" => vec!["artifact review", "inline annotations", "review code"],
            "Snip" => vec!["trim history", "compact context", "remove old messages"],
            "RemoteTrigger" => vec!["remote allthecodes", "trigger peer", "daemon submit"],
            "ListPeers" => vec!["discover peers", "daemon peers", "remote sessions"],
            "SubscribePR" => vec!["github pr", "pull request events", "webhook subscribe"],
            "WebFetch" => vec!["fetch url", "open webpage", "download page", "browse url"],
            "WebSearch" => vec!["internet search", "search web", "latest information"],
            "Agent" | "Task" => vec!["subagent", "delegate", "background agent", "task agent"],
            "Skill" => vec!["workflow", "slash skill", "skill invocation"],
            "ToolSearch" => vec!["discover tools", "search tools", "tool retrieval"],
            "SearchExtraTools" => vec![
                "deferred tools",
                "search extra tools",
                "discover hidden tools",
                "lazy tools",
            ],
            "ExecuteExtraTool" => vec![
                "execute deferred tool",
                "run extra tool",
                "lazy tool execution",
            ],
            "LSP" => vec!["language server", "diagnostics", "definition", "symbols"],
            "AskUserQuestion" => vec!["ask user", "clarify", "question"],
            "SystemStatus" => vec!["status", "subsystems", "health"],
            _ => Vec::new(),
        })
        .into_iter()
        .map(str::to_string),
    );

    if source == ToolSearchSource::Mcp {
        aliases.push(
            name.strip_prefix("mcp__")
                .unwrap_or(name)
                .replace("__", " "),
        );
    }

    aliases
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn normalize_query(raw: &str) -> NormalizedQuery {
    let trimmed = raw.trim();
    let (is_select, body) = trimmed
        .strip_prefix("select:")
        .or_else(|| trimmed.strip_prefix("SELECT:"))
        .map(|rest| (true, rest.trim()))
        .unwrap_or((false, trimmed));

    let mut tokens = tokenize_and_stem(body);
    let expansions = tokens
        .iter()
        .flat_map(|token| query_aliases(token).iter().copied())
        .map(str::to_string)
        .collect::<Vec<_>>();
    tokens.extend(expansions.into_iter().map(|t| stem(&t)));
    tokens = dedupe_strings(tokens);

    NormalizedQuery {
        phrase: normalize_phrase(body),
        tokens,
        is_select,
    }
}

fn query_aliases(token: &str) -> &'static [&'static str] {
    match token {
        "find" | "locate" | "lookup" => &["search", "grep", "glob"],
        "open" | "load" | "fetch" | "download" | "browse" | "url" | "page" => {
            &["read", "webfetch", "web"]
        }
        "edit" | "modify" | "patch" | "replace" | "update" => &["edit", "write"],
        "run" | "execute" | "shell" | "terminal" | "cmd" | "command" => {
            &["bash", "powershell", "repl"]
        }
        "ask" | "question" | "clarify" => &["askuserquestion"],
        "plan" | "planning" => &["enterplanmode", "exitplanmode"],
        "task" | "todo" | "background" => &["agent", "taskcreate", "tasklist"],
        _ => &[],
    }
}

fn tokenize_and_stem(text: &str) -> Vec<String> {
    dedupe_strings(tokenize(text).into_iter().map(|t| stem(&t)).collect())
}

fn tokenize(text: &str) -> Vec<String> {
    let normalized = normalize_phrase(text);
    let chars = normalized.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if is_cjk(ch) {
            let mut run = String::new();
            while i < chars.len() && is_cjk(chars[i]) {
                run.push(chars[i]);
                i += 1;
            }
            let run_chars = run.chars().collect::<Vec<_>>();
            if run_chars.len() == 1 {
                tokens.push(run);
            } else {
                for pair in run_chars.windows(2) {
                    tokens.push(pair.iter().collect());
                }
            }
        } else if ch.is_ascii_alphanumeric() {
            let mut word = String::new();
            while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                word.push(chars[i]);
                i += 1;
            }
            if !word.is_empty() && !STOP_WORDS.contains(&word.as_str()) {
                tokens.push(word);
            }
        } else {
            i += 1;
        }
    }

    tokens
}

fn normalize_phrase(text: &str) -> String {
    let mut out = String::new();
    let mut prev: Option<char> = None;

    for ch in text.chars() {
        if let Some(prev_ch) = prev {
            if (prev_ch.is_ascii_lowercase() || prev_ch.is_ascii_digit()) && ch.is_ascii_uppercase()
            {
                out.push(' ');
            }
        }

        if ch.is_ascii_alphanumeric() || is_cjk(ch) {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(' ');
        }
        prev = Some(ch);
    }

    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_identity(text: &str) -> String {
    normalize_phrase(text).replace(' ', "")
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x4dbf | 0x4e00..=0x9fff)
}

fn stem(word: &str) -> String {
    if word.chars().next().is_some_and(is_cjk) {
        return word.to_string();
    }
    let mut s = word.to_string();
    if s.ends_with("ing") && s.len() > 5 {
        s.truncate(s.len() - 3);
    } else if ["tion", "ness", "ment"]
        .iter()
        .any(|suffix| s.ends_with(suffix) && s.len() > 5)
    {
        s.truncate(s.len() - 4);
    } else if s.ends_with("ers") && s.len() > 4 {
        s.truncate(s.len() - 1);
    } else if s.ends_with("er") && s.len() > 4 {
        s.truncate(s.len() - 2);
    } else if s.ends_with("ies") && s.len() > 4 {
        s.truncate(s.len() - 3);
        s.push('y');
    } else if s.ends_with("les") && s.len() > 4 {
        s.truncate(s.len() - 1);
    } else if s.ends_with("es") && s.len() > 4 {
        s.truncate(s.len() - 2);
    } else if s.ends_with('s') && s.len() > 3 && !s.ends_with("ss") {
        s.truncate(s.len() - 1);
    } else if ["ed", "ly"]
        .iter()
        .any(|suffix| s.ends_with(suffix) && s.len() > 4)
    {
        s.truncate(s.len() - 2);
    }
    s
}

fn weighted_tf(fields: &[(&Vec<String>, f64)]) -> BTreeMap<String, f64> {
    let mut weighted = BTreeMap::new();
    for (tokens, weight) in fields {
        let mut freq = BTreeMap::<String, usize>::new();
        for token in tokens.iter() {
            *freq.entry(token.clone()).or_default() += 1;
        }
        let max = freq.values().copied().max().unwrap_or(1) as f64;
        for (term, count) in freq {
            let val = (count as f64 / max) * weight;
            let existing = weighted.entry(term).or_insert(0.0);
            if val > *existing {
                *existing = val;
            }
        }
    }
    weighted
}

fn base_vector(doc: &ToolSearchDocument) -> BTreeMap<String, f64> {
    weighted_tf(&[
        (&doc.name_tokens, 3.5),
        (&doc.alias_tokens, 2.5),
        (&doc.tag_tokens, 1.6),
        (&doc.description_tokens, 1.0),
    ])
}

fn compute_idf(docs: &[ToolSearchDocument]) -> BTreeMap<String, f64> {
    let mut df = BTreeMap::<String, usize>::new();
    for doc in docs {
        for term in doc.vector.keys().cloned().collect::<BTreeSet<_>>() {
            *df.entry(term).or_default() += 1;
        }
    }

    let n = docs.len() as f64;
    df.into_iter()
        .map(|(term, count)| {
            let idf = ((n + 1.0) / (count as f64 + 1.0)).ln() + 1.0;
            (term, idf)
        })
        .collect()
}

fn query_vector(tokens: &[String], idf: &BTreeMap<String, f64>) -> BTreeMap<String, f64> {
    let mut freq = BTreeMap::<String, usize>::new();
    for token in tokens {
        *freq.entry(token.clone()).or_default() += 1;
    }
    let max = freq.values().copied().max().unwrap_or(1) as f64;
    freq.into_iter()
        .map(|(term, count)| {
            let tf = count as f64 / max;
            (term.clone(), tf * idf.get(&term).copied().unwrap_or(1.0))
        })
        .collect()
}

fn cosine_similarity(query: &BTreeMap<String, f64>, doc: &BTreeMap<String, f64>) -> f64 {
    let dot = query
        .iter()
        .map(|(term, q)| q * doc.get(term).copied().unwrap_or(0.0))
        .sum::<f64>();
    let norm_q = query.values().map(|v| v * v).sum::<f64>().sqrt();
    let norm_d = doc.values().map(|v| v * v).sum::<f64>().sqrt();
    if norm_q == 0.0 || norm_d == 0.0 {
        0.0
    } else {
        dot / (norm_q * norm_d)
    }
}

fn overlap_score(
    query_tokens: &[String],
    doc_tokens: &[String],
    weight: f64,
    matched: &mut BTreeSet<String>,
) -> f64 {
    if doc_tokens.is_empty() {
        return 0.0;
    }
    let doc_set = doc_tokens.iter().collect::<HashSet<_>>();
    let mut score = 0.0;
    for token in query_tokens {
        if doc_set.contains(token) {
            score += weight;
            matched.insert(token.clone());
        }
    }
    score
}

fn extract_schema_terms(schema: &Value) -> Vec<String> {
    let mut text = Vec::new();
    collect_schema_text(schema, &mut text, 0);
    tokenize_and_stem(&text.join(" "))
}

fn collect_schema_text(value: &Value, out: &mut Vec<String>, depth: usize) {
    if depth > 8 || out.len() > 256 {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                out.push(key.clone());
                if matches!(
                    key.as_str(),
                    "description" | "title" | "type" | "enum" | "default"
                ) {
                    if let Some(s) = value.as_str() {
                        out.push(s.to_string());
                    }
                }
                collect_schema_text(value, out, depth + 1);
            }
        }
        Value::Array(items) => {
            for item in items.iter().take(32) {
                collect_schema_text(item, out, depth + 1);
            }
        }
        Value::String(s) => out.push(s.clone()),
        _ => {}
    }
}

fn is_schema_probe_term(token: &str) -> bool {
    matches!(
        token,
        "schema"
            | "input"
            | "parameter"
            | "argument"
            | "arg"
            | "field"
            | "path"
            | "file"
            | "url"
            | "pattern"
            | "regex"
            | "command"
            | "timeout"
            | "query"
            | "prompt"
            | "subsystem"
            | "skill"
            | "task"
            | "content"
            | "model"
            | "branch"
            | "cwd"
            | "server"
            | "format"
            | "message"
            | "question"
    )
}

fn sort_scored(scored: &mut [ScoredDocument<'_>]) {
    scored.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.doc.source.priority().cmp(&b.doc.source.priority()))
            .then_with(|| {
                normalize_identity(&a.doc.display_name)
                    .cmp(&normalize_identity(&b.doc.display_name))
            })
            .then_with(|| a.doc.ordinal.cmp(&b.doc.ordinal))
    });
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn dedupe_reasons(reasons: Vec<String>) -> Vec<String> {
    dedupe_strings(reasons)
}

fn round_score(score: f64) -> f64 {
    (score * 1000.0).round() / 1000.0
}

const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "could", "should", "may", "might", "shall", "can",
    "need", "to", "of", "in", "for", "on", "with", "at", "by", "from", "as", "into", "through",
    "during", "before", "after", "above", "below", "between", "out", "off", "over", "under",
    "again", "then", "once", "here", "there", "when", "where", "why", "how", "all", "each",
    "every", "both", "few", "more", "most", "other", "some", "such", "no", "nor", "not", "only",
    "own", "same", "so", "than", "too", "very", "just", "because", "but", "and", "or", "if",
    "while", "this", "that", "these", "those", "it", "its", "i", "me", "my", "we", "our", "you",
    "your", "he", "him", "his", "she", "her", "they", "them", "their", "what", "which", "who",
    "whom", "use", "using", "used",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ValidationResult;
    use allthecodes_skills::{SkillContext, SkillDefinition, SkillFrontmatter, SkillSource};
    use allthecodes_types::message::AssistantMessage;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone)]
    struct FixtureTool {
        name: &'static str,
        description: &'static str,
        schema: Value,
        schema_calls: Arc<AtomicUsize>,
        enabled: bool,
        read_only: bool,
    }

    impl FixtureTool {
        fn new(name: &'static str, description: &'static str, schema: Value) -> Self {
            Self {
                name,
                description,
                schema,
                schema_calls: Arc::new(AtomicUsize::new(0)),
                enabled: true,
                read_only: true,
            }
        }

        fn disabled(mut self) -> Self {
            self.enabled = false;
            self
        }
    }

    #[async_trait]
    impl Tool for FixtureTool {
        fn name(&self) -> &str {
            self.name
        }

        async fn description(&self, _input: &Value) -> String {
            self.description.to_string()
        }

        fn input_json_schema(&self) -> Value {
            self.schema_calls.fetch_add(1, Ordering::SeqCst);
            self.schema.clone()
        }

        fn is_enabled(&self) -> bool {
            self.enabled
        }

        fn is_read_only(&self, _input: &Value) -> bool {
            self.read_only
        }

        async fn validate_input(&self, _input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
            ValidationResult::Ok
        }

        async fn call(
            &self,
            _input: Value,
            _ctx: &ToolUseContext,
            _parent_message: &AssistantMessage,
            _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
        ) -> Result<ToolResult> {
            Ok(ToolResult::default())
        }

        async fn prompt(&self) -> String {
            self.description.to_string()
        }
    }

    async fn fixture_index(tools: Vec<FixtureTool>) -> (ToolSearchIndex, Vec<FixtureTool>) {
        let cloned = tools.clone();
        let tools = tools
            .into_iter()
            .map(|tool| Arc::new(tool) as Arc<dyn Tool>)
            .collect();
        (ToolSearchIndex::from_tools(tools).await, cloned)
    }

    #[test]
    fn query_normalization_handles_case_punctuation_and_aliases() {
        let q = normalize_query("Find files by PATH!");

        assert!(q.tokens.contains(&"find".to_string()));
        assert!(q.tokens.contains(&"file".to_string()));
        assert!(q.tokens.contains(&"path".to_string()));
        assert!(q.tokens.contains(&"grep".to_string()));
        assert!(q.tokens.contains(&"glob".to_string()));
        assert_eq!(q.phrase, "find files by path");
    }

    #[tokio::test]
    async fn ranking_prefers_exact_name_then_weighted_text() {
        let (index, _) = fixture_index(vec![
            FixtureTool::new(
                "Read",
                "Read file contents from disk.",
                json!({"type":"object","properties":{"file_path":{"type":"string"}}}),
            ),
            FixtureTool::new(
                "Grep",
                "Search file contents with a regex pattern.",
                json!({"type":"object","properties":{"pattern":{"type":"string"}}}),
            ),
            FixtureTool::new(
                "WebFetch",
                "Fetch a web page and extract text.",
                json!({"type":"object","properties":{"url":{"type":"string"}}}),
            ),
        ])
        .await;

        let read_results = index.search("read", ToolSearchOptions::default());
        assert_eq!(read_results[0].name, "Read");

        let regex_results = index.search("regex pattern", ToolSearchOptions::default());
        assert_eq!(regex_results[0].name, "Grep");
        assert!(regex_results[0].score > 0.0);
    }

    #[tokio::test]
    async fn schema_terms_are_loaded_only_when_ranking_needs_them() {
        let (index, tools) = fixture_index(vec![
            FixtureTool::new(
                "Read",
                "Read file contents from disk.",
                json!({
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "Absolute path to read"
                        }
                    }
                }),
            ),
            FixtureTool::new(
                "WebFetch",
                "Fetch a web page.",
                json!({
                    "type": "object",
                    "properties": {
                        "url": { "type": "string" }
                    }
                }),
            ),
        ])
        .await;

        let results = index.search("read file", ToolSearchOptions::default());
        assert_eq!(results[0].name, "Read");
        assert_eq!(tools[0].schema_calls.load(Ordering::SeqCst), 0);
        assert_eq!(tools[1].schema_calls.load(Ordering::SeqCst), 0);

        let results = index.search("absolute path", ToolSearchOptions::default());
        assert_eq!(results[0].name, "Read");
        assert!(results[0].schema_loaded);
        assert!(tools[0].schema_calls.load(Ordering::SeqCst) > 0);
    }

    #[tokio::test]
    async fn disabled_tools_are_excluded() {
        let (index, _) = fixture_index(vec![
            FixtureTool::new("Visible", "Visible search target.", json!({})),
            FixtureTool::new("Hidden", "Hidden search target.", json!({})).disabled(),
        ])
        .await;

        let results = index.search("hidden", ToolSearchOptions::default());
        assert!(results.iter().all(|r| r.name != "Hidden"));
    }

    #[tokio::test]
    async fn source_filter_handles_plugin_and_mcp_entries() {
        let docs = vec![
            DocumentInit {
                name: "deploy_status".to_string(),
                display_name: "deploy_status".to_string(),
                description: "Plugin tool that reads deployment status.".to_string(),
                source: ToolSearchSource::Plugin,
                category: "plugin".to_string(),
                tags: vec!["deploy".to_string()],
                aliases: vec!["deployment status".to_string()],
                invocation: None,
                ordinal: 0,
                schema_loader: None,
            },
            DocumentInit {
                name: "screenshot".to_string(),
                display_name: "mcp__browser__screenshot".to_string(),
                description: "Capture a browser screenshot.".to_string(),
                source: ToolSearchSource::Mcp,
                category: "mcp".to_string(),
                tags: vec!["browser".to_string()],
                aliases: vec!["browser screenshot".to_string()],
                invocation: None,
                ordinal: 1,
                schema_loader: None,
            },
        ];
        let index = ToolSearchIndex::new(docs.into_iter().map(ToolSearchDocument::new).collect());

        let plugin_results = index.search(
            "deployment status",
            ToolSearchOptions {
                source: Some(ToolSearchSource::Plugin),
                ..Default::default()
            },
        );
        assert_eq!(plugin_results[0].source, ToolSearchSource::Plugin);

        let mcp_results = index.search(
            "browser screenshot",
            ToolSearchOptions {
                source: Some(ToolSearchSource::Mcp),
                ..Default::default()
            },
        );
        assert_eq!(mcp_results[0].display_name, "mcp__browser__screenshot");
    }

    #[tokio::test]
    async fn large_catalog_name_query_does_not_hydrate_schemas() {
        let mut tools = Vec::new();
        for i in 0..600 {
            let name = if i == 427 { "TargetTool" } else { "FillerTool" };
            let leaked_name: &'static str = Box::leak(format!("{}{}", name, i).into_boxed_str());
            tools.push(FixtureTool::new(
                leaked_name,
                "Generic catalog entry.",
                json!({
                    "type": "object",
                    "properties": {
                        "expensive_field": { "type": "string" }
                    }
                }),
            ));
        }

        let (index, tools) = fixture_index(tools).await;
        let results = index.search("TargetTool427", ToolSearchOptions::default());

        assert_eq!(results[0].name, "TargetTool427");
        let schema_calls = tools
            .iter()
            .map(|tool| tool.schema_calls.load(Ordering::SeqCst))
            .sum::<usize>();
        assert_eq!(schema_calls, 0);
    }

    #[tokio::test]
    async fn tie_breaking_is_stable_by_source_and_name() {
        let docs = vec![
            DocumentInit {
                name: "zeta".to_string(),
                display_name: "zeta".to_string(),
                description: "shared match".to_string(),
                source: ToolSearchSource::Plugin,
                category: "plugin".to_string(),
                tags: vec![],
                aliases: vec![],
                invocation: None,
                ordinal: 0,
                schema_loader: None,
            },
            DocumentInit {
                name: "alpha".to_string(),
                display_name: "alpha".to_string(),
                description: "shared match".to_string(),
                source: ToolSearchSource::Plugin,
                category: "plugin".to_string(),
                tags: vec![],
                aliases: vec![],
                invocation: None,
                ordinal: 1,
                schema_loader: None,
            },
        ];
        let index = ToolSearchIndex::new(docs.into_iter().map(ToolSearchDocument::new).collect());

        let results = index.search("shared match", ToolSearchOptions::default());
        assert_eq!(results[0].name, "alpha");
        assert_eq!(results[1].name, "zeta");
    }

    #[tokio::test]
    async fn select_exact_hydrates_schema_for_selected_tool() {
        let (index, tools) = fixture_index(vec![FixtureTool::new(
            "Read",
            "Read file contents from disk.",
            json!({"type":"object","properties":{"file_path":{"type":"string"}}}),
        )])
        .await;

        let results = index.search("select:Read", ToolSearchOptions::default());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Read");
        assert!(results[0].schema_loaded);
        assert!(results[0].input_schema.is_some());
        assert_eq!(tools[0].schema_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn skills_are_indexed_as_skill_invocations() {
        allthecodes_skills::clear_skills();
        allthecodes_skills::register_skill(SkillDefinition {
            name: "review-code".to_string(),
            source: SkillSource::Project,
            base_dir: None,
            frontmatter: SkillFrontmatter {
                description: "Review code for correctness and regressions.".to_string(),
                when_to_use: Some("When the user asks for code review.".to_string()),
                context: SkillContext::Inline,
                ..Default::default()
            },
            prompt_body: "Review the code.".to_string(),
        });

        let index = ToolSearchIndex::from_tools(Vec::new())
            .await
            .with_skills(allthecodes_skills::get_model_invocable_skills());
        let results = index.search(
            "code review",
            ToolSearchOptions {
                source: Some(ToolSearchSource::Skill),
                ..Default::default()
            },
        );

        assert_eq!(results[0].name, "skill:review-code");
        assert_eq!(
            results[0].invocation,
            Some(json!({"tool":"Skill","input":{"skill":"review-code"}}))
        );
        allthecodes_skills::clear_skills();
    }
}
