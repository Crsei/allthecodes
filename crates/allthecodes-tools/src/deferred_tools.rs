//! Deferred tool discovery and execution helpers.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::{json, Value};

use crate::tool::{
    DeferredToolExecutionRequest, Tool, ToolProgress, ToolResult, ToolUseContext, Tools,
    ValidationResult,
};
use allthecodes_types::message::{
    AssistantMessage, Attachment, ContentBlock, Message, MessageContent, SystemSubtype,
    ToolResultContent,
};

const DEFAULT_MAX_RESULTS: usize = 5;
const MAX_RESULTS: usize = 25;

static CORE_TOOLS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "Agent",
        "AskUserQuestion",
        "Bash",
        "Brief",
        "Config",
        "Edit",
        "EnterPlanMode",
        "ExitPlanMode",
        "Glob",
        "Grep",
        "LSP",
        "ListMcpResources",
        "NotebookEdit",
        "Read",
        "ReadMcpResource",
        "SearchExtraTools",
        "SendUserMessage",
        "Skill",
        "Sleep",
        "StructuredOutput",
        "SystemStatus",
        "Task",
        "TaskCreate",
        "TaskGet",
        "TaskList",
        "TaskOutput",
        "TaskStop",
        "TaskUpdate",
        "TodoWrite",
        "ToolSearch",
        "WebFetch",
        "WebSearch",
        "Write",
        "ExecuteExtraTool",
    ]
    .into_iter()
    .collect()
});

static DISCOVERED_TOOLS: LazyLock<RwLock<HashMap<String, HashSet<String>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

#[derive(Debug, Clone)]
struct DeferredMatch {
    name: String,
    description: String,
    prompt: String,
    mcp_server_name: Option<String>,
    schema_index: Vec<String>,
    name_tokens: Vec<String>,
    score: usize,
    schema: Option<Value>,
}

pub struct SearchExtraToolsTool;
pub struct ExecuteExtraToolTool;

pub fn tools() -> Tools {
    vec![
        std::sync::Arc::new(SearchExtraToolsTool),
        std::sync::Arc::new(ExecuteExtraToolTool),
    ]
}

pub fn core_tool_names() -> &'static HashSet<&'static str> {
    &CORE_TOOLS
}

pub fn is_deferred_tool(name: &str) -> bool {
    !CORE_TOOLS.contains(name)
}

pub fn discovered_tools_for_session(session_id: &str) -> HashSet<String> {
    DISCOVERED_TOOLS
        .read()
        .get(session_id)
        .cloned()
        .unwrap_or_default()
}

pub fn mark_discovered_tools(session_id: &str, names: impl IntoIterator<Item = String>) {
    let mut discovered = DISCOVERED_TOOLS.write();
    let entry = discovered.entry(session_id.to_string()).or_default();
    entry.extend(names.into_iter().filter(|name| is_deferred_tool(name)));
}

pub fn clear_discovered_tools_for_tests() {
    DISCOVERED_TOOLS.write().clear();
}

pub fn extract_discovered_tool_names(messages: &[Message]) -> HashSet<String> {
    let mut names = HashSet::new();
    for message in messages {
        match message {
            Message::User(user) => {
                if let MessageContent::Blocks(blocks) = &user.content {
                    for block in blocks {
                        if let ContentBlock::ToolResult { content, .. } = block {
                            extract_from_tool_result_content(content, &mut names);
                        }
                    }
                }
            }
            Message::System(system) => {
                if let SystemSubtype::CompactBoundary {
                    compact_metadata: Some(metadata),
                } = &system.subtype
                {
                    if let Some(discovered) = &metadata.pre_compact_discovered_tools {
                        names.extend(discovered.iter().cloned());
                    }
                }
            }
            Message::Attachment(attachment) => {
                if let Attachment::StructuredOutput { data } = &attachment.attachment {
                    extract_names_from_value(data, &mut names);
                }
            }
            Message::Assistant(_) | Message::Progress(_) => {}
        }
    }
    names.retain(|name| is_deferred_tool(name));
    names
}

pub fn annotate_compact_boundaries_with_discovered_tools(
    messages: &mut [Message],
    discovered: &HashSet<String>,
) {
    if discovered.is_empty() {
        return;
    }
    let discovered = discovered
        .iter()
        .filter(|name| is_deferred_tool(name))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if discovered.is_empty() {
        return;
    }
    for message in messages {
        let Message::System(system) = message else {
            continue;
        };
        let SystemSubtype::CompactBoundary {
            compact_metadata: Some(metadata),
        } = &mut system.subtype
        else {
            continue;
        };
        metadata.pre_compact_discovered_tools = Some(discovered.clone());
    }
}

pub fn filter_tools_for_deferred_request(
    tools: Tools,
    messages: &[Message],
    session_id: &str,
) -> Tools {
    let mut discovered = discovered_tools_for_session(session_id);
    discovered.extend(extract_discovered_tool_names(messages));
    mark_discovered_tools(session_id, discovered.iter().cloned());

    tools
        .into_iter()
        .filter(|tool| CORE_TOOLS.contains(tool.name()))
        .collect()
}

fn extract_from_tool_result_content(content: &ToolResultContent, names: &mut HashSet<String>) {
    match content {
        ToolResultContent::Text(text) => {
            if let Ok(value) = serde_json::from_str::<Value>(text) {
                extract_names_from_value(&value, names);
            }
            for marker in ["Discovered deferred tools:", "deferred_tools_delta:"] {
                if let Some(rest) = text.split(marker).nth(1) {
                    for part in rest.split([',', '\n']) {
                        let name = part.trim().trim_matches(['`', '"', '[', ']']);
                        if !name.is_empty() {
                            names.insert(name.to_string());
                        }
                    }
                }
            }
        }
        ToolResultContent::Blocks(blocks) => {
            for block in blocks {
                if let ContentBlock::Text { text } = block {
                    extract_from_tool_result_content(&ToolResultContent::Text(text.clone()), names);
                }
            }
        }
    }
}

fn extract_names_from_value(value: &Value, names: &mut HashSet<String>) {
    if value
        .get("discovery_mode")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return;
    }
    if let Some(delta) = value.get("deferred_tools_delta") {
        extract_match_names(delta, names);
        return;
    }
    for key in [
        "pre_compact_discovered_tools",
        "tool_reference",
        "tool_references",
    ] {
        if let Some(value) = value.get(key) {
            extract_match_names(value, names);
        }
    }
    if value.get("total_deferred_tools").is_some() {
        if let Some(matches) = value.get("matches") {
            extract_match_names(matches, names);
        }
    }
}

fn extract_match_names(value: &Value, names: &mut HashSet<String>) {
    if let Some(name) = value.as_str() {
        names.insert(name.to_string());
        return;
    }
    if let Some(name) = value
        .get("name")
        .or_else(|| value.get("tool_name"))
        .and_then(Value::as_str)
    {
        names.insert(name.to_string());
        return;
    }
    if let Some(items) = value.as_array() {
        for item in items {
            extract_match_names(item, names);
        }
    }
}

fn max_results(input: &Value) -> Result<usize> {
    let max = input
        .get("max_results")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_MAX_RESULTS as u64);
    if max == 0 || max > MAX_RESULTS as u64 {
        bail!("max_results must be between 1 and {MAX_RESULTS}");
    }
    Ok(max as usize)
}

fn query_text(input: &Value) -> Result<String> {
    input
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("query is required"))
}

async fn deferred_candidates(tools: &Tools, include_schema: bool) -> Vec<DeferredMatch> {
    let mut names = BTreeSet::new();
    let mut out = Vec::new();
    for tool in tools {
        let name = tool.name().to_string();
        if !tool.is_enabled() || !is_deferred_tool(&name) || !names.insert(name.clone()) {
            continue;
        }
        let description = tool.description(&json!({})).await;
        let prompt = tool.prompt().await;
        let schema = tool.input_json_schema();
        let mut schema_index = Vec::new();
        collect_schema_index_terms(&schema, &mut schema_index);
        let mcp_server_name = tool.mcp_server_name().map(ToOwned::to_owned);
        let name_tokens = identifier_tokens(&name);
        if let Some(server) = &mcp_server_name {
            schema_index.extend(identifier_tokens(server));
            schema_index.push(server.clone());
        }
        out.push(DeferredMatch {
            name,
            description,
            prompt,
            mcp_server_name,
            schema_index,
            name_tokens,
            score: 0,
            schema: include_schema.then_some(schema),
        });
    }
    out
}

fn selected_names(query: &str) -> Option<Vec<String>> {
    query
        .strip_prefix("select:")
        .or_else(|| query.strip_prefix("SELECT:"))
        .map(|rest| {
            rest.split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
}

fn discover_query(query: &str) -> (&str, bool) {
    query
        .strip_prefix("discover:")
        .or_else(|| query.strip_prefix("DISCOVER:"))
        .map(|rest| (rest.trim(), true))
        .unwrap_or((query, false))
}

fn identifier_tokens(input: &str) -> Vec<String> {
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

fn collect_schema_index_terms(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if key == "properties" {
                    if let Some(properties) = value.as_object() {
                        for property in properties.keys() {
                            out.push(property.to_ascii_lowercase());
                            out.extend(identifier_tokens(property));
                        }
                    }
                }
                if matches!(key.as_str(), "description" | "title" | "searchHint") {
                    if let Some(text) = value.as_str() {
                        out.push(text.to_ascii_lowercase());
                    }
                }
                collect_schema_index_terms(value, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_schema_index_terms(item, out);
            }
        }
        Value::String(text) => {
            out.push(text.to_ascii_lowercase());
            out.extend(identifier_tokens(text));
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn candidate_index_text(candidate: &DeferredMatch) -> String {
    let mut fields = vec![
        candidate.name.clone(),
        candidate.description.clone(),
        candidate.prompt.clone(),
        candidate.name_tokens.join(" "),
        candidate.schema_index.join(" "),
    ];
    if let Some(server) = &candidate.mcp_server_name {
        fields.push(server.clone());
    }
    fields.join("\n").to_ascii_lowercase()
}

fn score_candidate(candidate: &DeferredMatch, query: &str) -> usize {
    let query = query.trim().to_ascii_lowercase();
    let terms = query
        .split_whitespace()
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return 0;
    }
    let name_lower = candidate.name.to_ascii_lowercase();
    let server_lower = candidate
        .mcp_server_name
        .as_deref()
        .map(str::to_ascii_lowercase);
    let haystack = candidate_index_text(candidate);
    let schema_terms = candidate
        .schema_index
        .iter()
        .map(|term| term.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let name_tokens = candidate
        .name_tokens
        .iter()
        .map(|term| term.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let mut score = 0;
    if query == name_lower {
        score += 500;
    }
    if server_lower.as_deref() == Some(query.as_str()) {
        score += 200;
    }
    for term in terms {
        if let Some(required) = term.strip_prefix('+') {
            if !haystack.contains(required) {
                return 0;
            }
            score += 30;
        } else if name_lower == term {
            score += 120;
        } else if server_lower.as_deref() == Some(term) {
            score += 90;
        } else if name_tokens.contains(term) {
            score += 60;
        } else if schema_terms.contains(term) {
            score += 45;
        } else if name_lower.contains(term) {
            score += 35;
        } else if server_lower
            .as_deref()
            .is_some_and(|server| server.contains(term))
        {
            score += 30;
        } else if haystack.contains(term) {
            score += 10;
        }
    }
    score
}

fn matches_to_json(matches: &[DeferredMatch]) -> Vec<Value> {
    matches
        .iter()
        .map(|item| {
            let mut value = json!({
                "name": item.name,
                "description": item.description,
                "score": item.score,
            });
            if let Some(server) = &item.mcp_server_name {
                value["mcp_server_name"] = json!(server);
            }
            if let Some(schema) = &item.schema {
                value["input_schema"] = schema.clone();
            }
            value
        })
        .collect()
}

#[async_trait]
impl Tool for SearchExtraToolsTool {
    fn name(&self) -> &str {
        "SearchExtraTools"
    }

    async fn description(&self, _input: &Value) -> String {
        "Search deferred tools that are not currently visible in the core tool set.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search text, select:<tool-name>[,<tool-name>], or discover:<query>."
                },
                "max_results": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_RESULTS,
                    "default": DEFAULT_MAX_RESULTS
                }
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
        match query_text(input).and_then(|_| max_results(input).map(|_| ())) {
            Ok(()) => ValidationResult::Ok,
            Err(err) => ValidationResult::Error {
                message: err.to_string(),
                error_code: 400,
            },
        }
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let raw_query = query_text(&input)?;
        let limit = max_results(&input)?;
        let (query, discover) = discover_query(&raw_query);
        let mut candidates = deferred_candidates(&ctx.available_tools, discover).await;
        let total_deferred_tools = candidates.len();

        let matches = if let Some(selected) = selected_names(&raw_query) {
            let selected = selected
                .into_iter()
                .map(|name| name.to_ascii_lowercase())
                .collect::<HashSet<_>>();
            candidates
                .into_iter()
                .filter_map(|mut candidate| {
                    if selected.contains(&candidate.name.to_ascii_lowercase()) {
                        candidate.score = 100;
                        Some(candidate)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        } else {
            for candidate in &mut candidates {
                candidate.score = score_candidate(candidate, query);
            }
            candidates
                .into_iter()
                .filter(|candidate| candidate.score > 0)
                .collect::<Vec<_>>()
        };

        let mut matches = matches;
        matches.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.name.cmp(&b.name)));
        matches.truncate(limit);

        let discovered_names = if discover {
            Vec::new()
        } else {
            matches
                .iter()
                .map(|item| item.name.clone())
                .collect::<Vec<_>>()
        };
        mark_discovered_tools(&ctx.session_id, discovered_names.clone());
        let already_loaded = discovered_tools_for_session(&ctx.session_id)
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let result_json = json!({
            "query": raw_query,
            "discovery_mode": discover,
            "matches": matches_to_json(&matches),
            "deferred_tools_delta": discovered_names,
            "total_deferred_tools": total_deferred_tools,
            "already_loaded": already_loaded,
        });
        let model_text = serde_json::to_string_pretty(&result_json)?;
        let preview = if matches.is_empty() {
            "No deferred tools matched".to_string()
        } else if discover {
            format!(
                "Matched deferred tools: {}",
                matches
                    .iter()
                    .map(|item| item.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            format!(
                "Discovered deferred tools: {}",
                matches
                    .iter()
                    .map(|item| item.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        Ok(ToolResult {
            data: result_json,
            model_content: Some(ToolResultContent::Text(model_text)),
            display_preview: Some(preview),
            new_messages: vec![],
        })
    }

    async fn prompt(&self) -> String {
        "Search deferred tools by name or intent. Use select:<tool-name> to mark a specific deferred tool as discovered, then execute it through ExecuteExtraTool. Use discover:<query> only to inspect descriptions and schemas without changing discovered state."
            .to_string()
    }
}

#[async_trait]
impl Tool for ExecuteExtraToolTool {
    fn name(&self) -> &str {
        "ExecuteExtraTool"
    }

    async fn description(&self, _input: &Value) -> String {
        "Execute a deferred tool that was previously discovered with SearchExtraTools.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tool_name": { "type": "string" },
                "params": {
                    "type": "object",
                    "description": "Input object passed to the deferred tool."
                }
            },
            "required": ["tool_name", "params"]
        })
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let has_tool = input
            .get("tool_name")
            .and_then(Value::as_str)
            .is_some_and(|name| !name.trim().is_empty());
        let has_params = input.get("params").is_some_and(Value::is_object);
        if has_tool && has_params {
            ValidationResult::Ok
        } else {
            ValidationResult::Error {
                message: "'tool_name' and object 'params' are required".to_string(),
                error_code: 400,
            }
        }
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let tool_name = input
            .get("tool_name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| anyhow!("tool_name is required"))?;
        if matches!(tool_name, "ExecuteExtraTool" | "SearchExtraTools") {
            bail!("{tool_name} cannot be executed through ExecuteExtraTool");
        }
        if !is_deferred_tool(tool_name) {
            bail!("{tool_name} is a core tool; call it directly instead of ExecuteExtraTool");
        }

        let mut discovered = discovered_tools_for_session(&ctx.session_id);
        discovered.extend(extract_discovered_tool_names(&ctx.messages));
        mark_discovered_tools(&ctx.session_id, discovered.iter().cloned());
        if !discovered.contains(tool_name) {
            bail!("use SearchExtraTools first to discover {tool_name}");
        }

        let params = input
            .get("params")
            .cloned()
            .filter(Value::is_object)
            .ok_or_else(|| anyhow!("params must be an object"))?;
        let target = ctx
            .available_tools
            .iter()
            .find(|tool| tool.name() == tool_name)
            .ok_or_else(|| anyhow!("deferred tool not found: {tool_name}"))?;
        if !target.is_enabled() {
            bail!("deferred tool is disabled: {tool_name}");
        }

        let execute = ctx
            .execute_deferred_tool
            .as_ref()
            .ok_or_else(|| anyhow!("deferred tool execution is unavailable in this context"))?;
        let result = execute(DeferredToolExecutionRequest {
            tool_use_id: format!("execute-extra-{tool_name}"),
            tool_name: tool_name.to_string(),
            input: params,
        })
        .await?;
        if result.is_error {
            bail!("{}", tool_result_error_text(&result.result));
        }

        Ok(ToolResult {
            data: json!({
                "tool_use_id": result.tool_use_id,
                "tool_name": result.tool_name,
                "result": result.result.data,
            }),
            model_content: result.result.model_content,
            display_preview: result
                .result
                .display_preview
                .or_else(|| Some(format!("Executed deferred tool {tool_name}"))),
            new_messages: result.result.new_messages,
        })
    }

    async fn prompt(&self) -> String {
        "Execute a deferred tool after SearchExtraTools has discovered it. Hidden deferred tools are not added to the visible schema; pass the exact target tool name and params through ExecuteExtraTool."
            .to_string()
    }
}

fn tool_result_error_text(result: &ToolResult) -> String {
    result.display_preview.clone().unwrap_or_else(|| {
        result
            .data
            .as_str()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| {
                serde_json::to_string(&result.data)
                    .unwrap_or_else(|_| "deferred tool failed".into())
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{
        Attachment, AttachmentMessage, CompactMetadata, ContentBlock, MessageContent,
        SystemMessage, UserMessage,
    };
    use std::sync::Arc;

    struct NamedTool(&'static str);

    #[async_trait]
    impl Tool for NamedTool {
        fn name(&self) -> &str {
            self.0
        }

        async fn description(&self, _input: &Value) -> String {
            format!("{} test tool", self.0)
        }

        fn input_json_schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {},
            })
        }

        async fn call(
            &self,
            _input: Value,
            _ctx: &ToolUseContext,
            _parent_message: &AssistantMessage,
            _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
        ) -> Result<ToolResult> {
            Ok(ToolResult {
                data: json!({ "tool": self.0 }),
                model_content: None,
                display_preview: None,
                new_messages: vec![],
            })
        }

        async fn prompt(&self) -> String {
            format!("{} test prompt", self.0)
        }
    }

    struct IndexedTool {
        name: &'static str,
        description: &'static str,
        prompt: &'static str,
        mcp_server_name: Option<&'static str>,
        schema: Value,
    }

    #[async_trait]
    impl Tool for IndexedTool {
        fn name(&self) -> &str {
            self.name
        }

        async fn description(&self, _input: &Value) -> String {
            self.description.to_string()
        }

        fn input_json_schema(&self) -> Value {
            self.schema.clone()
        }

        fn mcp_server_name(&self) -> Option<&str> {
            self.mcp_server_name
        }

        async fn call(
            &self,
            _input: Value,
            _ctx: &ToolUseContext,
            _parent_message: &AssistantMessage,
            _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
        ) -> Result<ToolResult> {
            Ok(ToolResult {
                data: json!({ "tool": self.name }),
                model_content: None,
                display_preview: None,
                new_messages: vec![],
            })
        }

        async fn prompt(&self) -> String {
            self.prompt.to_string()
        }
    }

    fn test_context(session_id: &str, available_tools: Tools) -> ToolUseContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolUseContext {
            options: crate::tool::ToolUseOptions {
                debug: false,
                main_loop_model: "test-model".to_string(),
                verbose: false,
                is_non_interactive_session: false,
                custom_system_prompt: None,
                append_system_prompt: None,
                max_budget_usd: None,
            },
            abort_signal: rx,
            read_file_state: crate::tool::FileStateCache::default(),
            get_app_state: Arc::new(crate::tool::ToolAppState::default),
            set_app_state: Arc::new(|_| {}),
            session_id: session_id.to_string(),
            langfuse_session_id: session_id.to_string(),
            messages: vec![],
            agent_id: None,
            agent_type: None,
            query_tracking: None,
            permission_callback: None,
            ask_user_callback: None,
            permission_event_callback: None,
            bg_agent_tx: None,
            hook_runner: Arc::new(allthecodes_types::hooks::NoopHookRunner::new()),
            command_dispatcher: Arc::new(allthecodes_types::commands::NoopCommandDispatcher::new()),
            available_tools,
            execute_deferred_tool: None,
        }
    }

    fn parent_message() -> AssistantMessage {
        AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1,
            role: "assistant".to_string(),
            content: vec![],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        }
    }

    #[test]
    fn core_boundary_marks_product_tools_deferred() {
        assert!(!is_deferred_tool("Read"));
        assert!(!is_deferred_tool("SearchExtraTools"));
        assert!(!is_deferred_tool("ExecuteExtraTool"));
        assert!(is_deferred_tool("CronCreate"));
        assert!(is_deferred_tool("WebBrowser"));
        assert!(is_deferred_tool("Workflow"));
        assert!(is_deferred_tool("LocalMemoryRecall"));
        assert!(is_deferred_tool("VaultHttpFetch"));
        assert!(is_deferred_tool("GetGoal"));
        assert!(is_deferred_tool("ViewImage"));
        assert!(is_deferred_tool("ListAgents"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn search_uses_runtime_catalog_and_marks_keyword_matches() {
        clear_discovered_tools_for_tests();
        let ctx = test_context(
            "search-keyword",
            vec![
                Arc::new(crate::sleep::SleepTool),
                Arc::new(NamedTool("RuntimeOnly")),
            ],
        );
        let result = SearchExtraToolsTool
            .call(
                json!({"query": "runtime", "max_results": 10}),
                &ctx,
                &parent_message(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.data["matches"][0]["name"], "RuntimeOnly");
        assert_eq!(result.data["deferred_tools_delta"], json!(["RuntimeOnly"]));
        assert!(discovered_tools_for_session("search-keyword").contains("RuntimeOnly"));
        assert!(!result.data["matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "Sleep"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn search_select_marks_and_discover_does_not_mark() {
        clear_discovered_tools_for_tests();
        let ctx = test_context(
            "search-select",
            vec![
                Arc::new(NamedTool("WebBrowser")),
                Arc::new(NamedTool("Workflow")),
            ],
        );

        let selected = SearchExtraToolsTool
            .call(
                json!({"query": "select:WebBrowser", "max_results": 10}),
                &ctx,
                &parent_message(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(selected.data["deferred_tools_delta"], json!(["WebBrowser"]));
        assert!(discovered_tools_for_session("search-select").contains("WebBrowser"));

        clear_discovered_tools_for_tests();
        let discover_ctx = test_context("search-discover", vec![Arc::new(NamedTool("Workflow"))]);
        let discovered = SearchExtraToolsTool
            .call(
                json!({"query": "discover:workflow", "max_results": 10}),
                &discover_ctx,
                &parent_message(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(discovered.data["deferred_tools_delta"], json!([]));
        assert!(discovered.data["matches"][0].get("input_schema").is_some());
        assert!(discovered_tools_for_session("search-discover").is_empty());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn search_ranks_exact_tool_names_ahead_of_partial_matches() {
        clear_discovered_tools_for_tests();
        let ctx = test_context(
            "search-exact-name",
            vec![
                Arc::new(IndexedTool {
                    name: "WebBrowserHelper",
                    description: "browser helper",
                    prompt: "open websites",
                    mcp_server_name: None,
                    schema: json!({"type": "object", "properties": {}}),
                }),
                Arc::new(IndexedTool {
                    name: "WebBrowser",
                    description: "open browser pages",
                    prompt: "navigate the browser",
                    mcp_server_name: None,
                    schema: json!({"type": "object", "properties": {}}),
                }),
            ],
        );

        let result = SearchExtraToolsTool
            .call(
                json!({"query": "WebBrowser", "max_results": 10}),
                &ctx,
                &parent_message(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.data["matches"][0]["name"], "WebBrowser");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn search_matches_mcp_server_names_and_schema_action_keywords() {
        clear_discovered_tools_for_tests();
        let ctx = test_context(
            "search-index-fields",
            vec![
                Arc::new(IndexedTool {
                    name: "AuditTrail",
                    description: "Inspect local audit trail entries",
                    prompt: "review local timeline",
                    mcp_server_name: None,
                    schema: json!({
                        "type": "object",
                        "properties": {
                            "since": {"type": "string"}
                        }
                    }),
                }),
                Arc::new(IndexedTool {
                    name: "mcp__github__create_pull_request",
                    description: "Create a pull request through an MCP tool",
                    prompt: "publish code review changes",
                    mcp_server_name: Some("github"),
                    schema: json!({
                        "type": "object",
                        "properties": {
                            "branch_name": {
                                "type": "string",
                                "description": "Source branch for the pull request"
                            },
                            "title": {"type": "string"}
                        }
                    }),
                }),
            ],
        );

        let mcp = SearchExtraToolsTool
            .call(
                json!({"query": "github", "max_results": 10}),
                &ctx,
                &parent_message(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            mcp.data["matches"][0]["name"],
            "mcp__github__create_pull_request"
        );
        assert_eq!(mcp.data["matches"][0]["mcp_server_name"], "github");

        let action = SearchExtraToolsTool
            .call(
                json!({"query": "branch", "max_results": 10}),
                &ctx,
                &parent_message(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            action.data["matches"][0]["name"],
            "mcp__github__create_pull_request"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn execute_extra_tool_requires_discovery_and_uses_deferred_executor() {
        clear_discovered_tools_for_tests();
        let mut ctx = test_context("execute-runtime", vec![Arc::new(NamedTool("RuntimeOnly"))]);

        let missing = ExecuteExtraToolTool
            .call(
                json!({"tool_name": "RuntimeOnly", "params": {}}),
                &ctx,
                &parent_message(),
                None,
            )
            .await
            .unwrap_err();
        assert!(missing.to_string().contains("use SearchExtraTools first"));

        mark_discovered_tools("execute-runtime", ["RuntimeOnly".to_string()]);
        let calls = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let calls_for_executor = calls.clone();
        ctx.execute_deferred_tool = Some(Arc::new(move |request| {
            calls_for_executor.lock().push(request.tool_name.clone());
            Box::pin(async move {
                Ok(crate::tool::DeferredToolExecutionResult {
                    tool_use_id: request.tool_use_id,
                    tool_name: request.tool_name,
                    result: ToolResult {
                        data: json!({"ok": true}),
                        display_preview: Some("runtime ok".to_string()),
                        ..Default::default()
                    },
                    is_error: false,
                })
            })
        }));

        let result = ExecuteExtraToolTool
            .call(
                json!({"tool_name": "RuntimeOnly", "params": {}}),
                &ctx,
                &parent_message(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(&*calls.lock(), &["RuntimeOnly".to_string()]);
        assert_eq!(result.data["tool_name"], "RuntimeOnly");
        assert_eq!(result.data["result"], json!({"ok": true}));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn execute_extra_tool_handles_mcp_style_runtime_targets() {
        clear_discovered_tools_for_tests();
        let target = "mcp__github__create_pull_request";
        let mut ctx = test_context(
            "execute-mcp-runtime",
            vec![Arc::new(IndexedTool {
                name: target,
                description: "Create a pull request through an MCP tool",
                prompt: "publish code review changes",
                mcp_server_name: Some("github"),
                schema: json!({
                    "type": "object",
                    "properties": {
                        "branch_name": {"type": "string"},
                        "title": {"type": "string"}
                    }
                }),
            })],
        );

        let discovered = SearchExtraToolsTool
            .call(
                json!({"query": "select:mcp__github__create_pull_request"}),
                &ctx,
                &parent_message(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(discovered.data["deferred_tools_delta"], json!([target]));

        let calls = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let calls_for_executor = calls.clone();
        ctx.execute_deferred_tool = Some(Arc::new(move |request| {
            calls_for_executor.lock().push(request.tool_name.clone());
            Box::pin(async move {
                Ok(crate::tool::DeferredToolExecutionResult {
                    tool_use_id: request.tool_use_id,
                    tool_name: request.tool_name,
                    result: ToolResult {
                        data: json!({"mcp": true}),
                        display_preview: Some("mcp runtime ok".to_string()),
                        ..Default::default()
                    },
                    is_error: false,
                })
            })
        }));

        let result = ExecuteExtraToolTool
            .call(
                json!({
                    "tool_name": target,
                    "params": {
                        "branch_name": "feature/phase5",
                        "title": "Phase 5"
                    }
                }),
                &ctx,
                &parent_message(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(&*calls.lock(), &[target.to_string()]);
        assert_eq!(result.data["tool_name"], target);
        assert_eq!(result.data["result"], json!({"mcp": true}));
    }

    #[test]
    fn extracts_discovered_names_from_tool_result_json() {
        let message = Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1,
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "toolu_1".to_string(),
                content: ToolResultContent::Text(
                    json!({
                        "total_deferred_tools": 2,
                        "matches": [
                            {"name": "CronCreate"},
                            "WebBrowser",
                            {"name": "Read"}
                        ]
                    })
                    .to_string(),
                ),
                is_error: false,
            }]),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        });

        let names = extract_discovered_tool_names(&[message]);
        assert!(names.contains("CronCreate"));
        assert!(names.contains("WebBrowser"));
        assert!(!names.contains("Read"));
    }

    #[test]
    fn discovery_mode_does_not_load_matches() {
        let message = Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1,
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "toolu_1".to_string(),
                content: ToolResultContent::Text(
                    json!({
                        "discovery_mode": true,
                        "total_deferred_tools": 1,
                        "matches": [{"name": "CronCreate"}],
                        "deferred_tools_delta": [],
                    })
                    .to_string(),
                ),
                is_error: false,
            }]),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        });

        let names = extract_discovered_tool_names(&[message]);
        assert!(names.is_empty());
    }

    #[test]
    fn extracts_from_attachment_and_compact_metadata() {
        let attachment = Message::Attachment(AttachmentMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1,
            attachment: Attachment::StructuredOutput {
                data: json!({
                    "deferred_tools_delta": ["CronCreate"],
                }),
            },
        });
        let boundary = Message::System(SystemMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 2,
            subtype: SystemSubtype::CompactBoundary {
                compact_metadata: Some(CompactMetadata {
                    pre_compact_token_count: 100,
                    post_compact_token_count: 50,
                    preserved_segment: None,
                    pre_compact_discovered_tools: Some(vec![
                        "WebBrowser".to_string(),
                        "Read".to_string(),
                    ]),
                }),
            },
            content: "compacted".to_string(),
        });

        let names = extract_discovered_tool_names(&[attachment, boundary]);
        assert!(names.contains("CronCreate"));
        assert!(names.contains("WebBrowser"));
        assert!(!names.contains("Read"));
    }

    #[test]
    fn annotates_compact_boundaries_with_sorted_deferred_names() {
        let mut messages = vec![Message::System(SystemMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1,
            subtype: SystemSubtype::CompactBoundary {
                compact_metadata: Some(CompactMetadata {
                    pre_compact_token_count: 10,
                    post_compact_token_count: 5,
                    preserved_segment: None,
                    pre_compact_discovered_tools: None,
                }),
            },
            content: "compacted".to_string(),
        })];
        annotate_compact_boundaries_with_discovered_tools(
            &mut messages,
            &[
                "Read".to_string(),
                "WebBrowser".to_string(),
                "CronCreate".to_string(),
            ]
            .into_iter()
            .collect(),
        );

        let Message::System(system) = &messages[0] else {
            panic!("expected compact boundary");
        };
        let SystemSubtype::CompactBoundary {
            compact_metadata: Some(metadata),
        } = &system.subtype
        else {
            panic!("expected metadata");
        };
        assert_eq!(
            metadata.pre_compact_discovered_tools.as_ref().unwrap(),
            &vec!["CronCreate".to_string(), "WebBrowser".to_string()]
        );
    }

    #[test]
    #[serial_test::serial]
    fn filter_keeps_only_core_tools_even_after_discovery() {
        clear_discovered_tools_for_tests();
        let tools: Tools = vec![
            std::sync::Arc::new(crate::sleep::SleepTool),
            std::sync::Arc::new(NamedTool("CronCreate")),
            std::sync::Arc::new(NamedTool("WebBrowser")),
        ];
        let names = |tools: Tools| {
            tools
                .into_iter()
                .map(|tool| tool.name().to_string())
                .collect::<BTreeSet<_>>()
        };

        let filtered = names(filter_tools_for_deferred_request(
            tools.clone(),
            &[],
            "session-a",
        ));
        assert_eq!(filtered, BTreeSet::from(["Sleep".to_string()]));

        mark_discovered_tools("session-b", ["CronCreate".to_string()]);
        let filtered = names(filter_tools_for_deferred_request(tools, &[], "session-b"));
        assert_eq!(filtered, BTreeSet::from(["Sleep".to_string()]));
        assert!(discovered_tools_for_session("session-b").contains("CronCreate"));
    }
}
