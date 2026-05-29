//! Deferred tool discovery and execution helpers.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::{json, Value};

use crate::registry;
use crate::tool::{
    PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, Tools, ValidationResult,
};
use allthecodes_types::callbacks::{PermissionRequestPayload, PermissionResponsePayload};
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
        .filter(|tool| {
            let name = tool.name();
            CORE_TOOLS.contains(name) || discovered.contains(name)
        })
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

async fn deferred_candidates(include_schema: bool) -> Vec<DeferredMatch> {
    let mut names = BTreeSet::new();
    let mut out = Vec::new();
    for tool in registry::get_all_tools() {
        let name = tool.name().to_string();
        if !is_deferred_tool(&name) || !names.insert(name.clone()) {
            continue;
        }
        let description = tool.description(&json!({})).await;
        out.push(DeferredMatch {
            name,
            description,
            score: 0,
            schema: include_schema.then(|| tool.input_json_schema()),
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

fn score_candidate(candidate: &DeferredMatch, query: &str) -> usize {
    let query = query.to_ascii_lowercase();
    let terms = query
        .split_whitespace()
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return 0;
    }
    let haystack = format!("{} {}", candidate.name, candidate.description).to_ascii_lowercase();
    let mut score = 0;
    for term in terms {
        if let Some(required) = term.strip_prefix('+') {
            if !haystack.contains(required) {
                return 0;
            }
            score += 5;
        } else if candidate.name.eq_ignore_ascii_case(term) {
            score += 20;
        } else if candidate.name.to_ascii_lowercase().contains(term) {
            score += 8;
        } else if haystack.contains(term) {
            score += 3;
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
        let mut candidates = deferred_candidates(discover).await;
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
        "Search deferred tools by name or intent. Use select:<tool-name> to load a specific deferred tool, then call it directly on a later turn or through ExecuteExtraTool."
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
        parent_message: &AssistantMessage,
        on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let tool_name = input
            .get("tool_name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| anyhow!("tool_name is required"))?;
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
        let tools = registry::get_all_tools();
        let target = tools
            .into_iter()
            .find(|tool| tool.name() == tool_name)
            .ok_or_else(|| anyhow!("deferred tool not found: {tool_name}"))?;
        if !target.is_enabled() {
            bail!("deferred tool is disabled: {tool_name}");
        }

        match target.validate_input(&params, ctx).await {
            ValidationResult::Ok => {}
            ValidationResult::Error { message, .. } => {
                bail!("Input validation error for {tool_name}: {message}");
            }
        }

        let effective_input = match target.check_permissions(&params, ctx).await {
            PermissionResult::Allow { updated_input } => updated_input,
            PermissionResult::Deny { message } => {
                bail!("Permission denied for {tool_name}: {message}")
            }
            PermissionResult::Ask { message } => {
                let Some(callback) = ctx.permission_callback.as_ref() else {
                    bail!("Permission required for {tool_name}: {message}");
                };
                let response = callback(PermissionRequestPayload {
                    tool_use_id: format!("execute-extra-{tool_name}"),
                    tool_name: tool_name.to_string(),
                    tool_input: params.clone(),
                    message,
                    options: vec![
                        "Allow".to_string(),
                        "Deny".to_string(),
                        "Always Allow".to_string(),
                    ],
                })
                .await;
                if !matches!(
                    response.normalized_decision().as_str(),
                    "allow" | "always_allow"
                ) {
                    bail!(
                        "Permission denied for {tool_name}: {}",
                        denial_message(&response)
                    );
                }
                params
            }
        };

        let result = target
            .call(effective_input, ctx, parent_message, on_progress)
            .await?;
        Ok(ToolResult {
            data: json!({
                "tool_name": tool_name,
                "result": result.data,
            }),
            model_content: result.model_content,
            display_preview: result
                .display_preview
                .or_else(|| Some(format!("Executed deferred tool {tool_name}"))),
            new_messages: result.new_messages,
        })
    }

    async fn prompt(&self) -> String {
        "Execute a deferred tool after SearchExtraTools has discovered it. Prefer calling the discovered tool directly if its schema is visible in the next request."
            .to_string()
    }
}

fn denial_message(response: &PermissionResponsePayload) -> String {
    response
        .feedback
        .clone()
        .unwrap_or_else(|| response.normalized_decision())
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{
        Attachment, AttachmentMessage, CompactMetadata, ContentBlock, MessageContent,
        SystemMessage, UserMessage,
    };

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

    #[test]
    fn core_boundary_marks_product_tools_deferred() {
        assert!(!is_deferred_tool("Read"));
        assert!(!is_deferred_tool("SearchExtraTools"));
        assert!(is_deferred_tool("CronCreate"));
        assert!(is_deferred_tool("WebBrowser"));
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
    fn filter_keeps_core_and_discovered_tools() {
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
        assert_eq!(
            filtered,
            BTreeSet::from(["CronCreate".to_string(), "Sleep".to_string()])
        );
    }
}
