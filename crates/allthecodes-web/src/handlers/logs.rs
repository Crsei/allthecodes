//! Logs and diagnostics handlers.
//!
//! These endpoints intentionally provide a bounded, best-effort view over
//! local development logs. They are useful without requiring a durable backend
//! diagnostics store, and they return empty-success responses when no trace
//! data has been captured yet.

use std::collections::{BTreeMap, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::state::WebState;

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 500;
const MAX_LINES_PER_SOURCE: usize = 10_000;
const BACKEND_DEV_LOG: &str = "/tmp/allthecodes-dev-restart/backend.log";
const FRONTEND_DEV_LOG: &str = "/tmp/allthecodes-dev-restart/frontend.log";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LogsQuery {
    pub profile_id: Option<String>,
    pub source: Option<String>,
    pub level: Option<String>,
    pub category: Option<String>,
    pub session_id: Option<String>,
    pub since: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
    pub search: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LogsExportQuery {
    pub format: Option<String>,
    pub profile_id: Option<String>,
    pub source: Option<String>,
    pub level: Option<String>,
    pub category: Option<String>,
    pub session_id: Option<String>,
    pub since: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
    pub search: Option<String>,
}

impl From<&LogsExportQuery> for LogsQuery {
    fn from(query: &LogsExportQuery) -> Self {
        Self {
            profile_id: query.profile_id.clone(),
            source: query.source.clone(),
            level: query.level.clone(),
            category: query.category.clone(),
            session_id: query.session_id.clone(),
            since: query.since.clone(),
            cursor: query.cursor.clone(),
            limit: query.limit,
            search: query.search.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DiagnosticsQuery {
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TracesQuery {
    pub profile_id: Option<String>,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogsResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub entries: Vec<LogEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub id: String,
    pub timestamp: i64,
    pub level: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_raw: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub timestamp: i64,
    pub frontend_event_count: usize,
    pub ipc_event_count: usize,
    pub renderer_state_count: usize,
    pub captured_sse_count: usize,
    pub captured_ws_count: usize,
    pub enabled: bool,
    pub events: Vec<DiagnosticsEvent>,
    pub session_traces: Vec<SessionTrace>,
    pub renderer_states: Vec<Value>,
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsEvent {
    pub timestamp: i64,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub message_id: Option<String>,
    pub event_type: String,
    pub payload_summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_raw: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionTrace {
    pub session_id: String,
    pub ownership: String,
    pub is_streaming: bool,
    pub current_turn: Option<Value>,
    pub pending_permissions: Vec<String>,
    pub pending_questions: Vec<String>,
    pub recent_events: Vec<DiagnosticsEvent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TracesResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub traces: Vec<TraceSummary>,
    pub detail: Option<TraceDetail>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceSummary {
    pub id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    pub event_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceDetail {
    pub id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    pub event_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub events: Vec<DiagnosticsEvent>,
    pub turns: Vec<Value>,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
struct LogSourceSpec {
    source: &'static str,
    path: PathBuf,
}

#[derive(Debug, Clone)]
struct CollectedLogs {
    entries: Vec<LogEntry>,
    source_truncated: bool,
}

/// GET /api/logs
pub async fn logs_handler(Query(query): Query<LogsQuery>) -> impl IntoResponse {
    let collected = collect_logs(&query);
    let page = page_entries(collected.entries, &query);

    Json(LogsResponse {
        profile_id: query.profile_id,
        entries: page.entries,
        next_cursor: page.next_cursor,
        truncated: collected.source_truncated || page.has_more,
        message: None,
    })
}

/// GET /api/logs/export
pub async fn logs_export_handler(Query(query): Query<LogsExportQuery>) -> Response {
    let logs_query = LogsQuery::from(&query);
    let mut entries = collect_logs(&logs_query).entries;
    entries = filter_entries(entries, &logs_query);

    let format = query.format.as_deref().unwrap_or("jsonl");
    let filename = format!(
        "allthecodes-logs-{}.{}",
        Local::now().format("%Y%m%d-%H%M%S"),
        if format == "json" { "json" } else { "jsonl" }
    );

    let (content_type, body) = if format == "json" {
        (
            "application/json",
            serde_json::to_string(&json!({ "entries": entries }))
                .unwrap_or_else(|_| "{\"entries\":[]}".to_string()),
        )
    } else {
        let mut out = String::new();
        for entry in entries {
            if let Ok(line) = serde_json::to_string(&entry) {
                out.push_str(&line);
                out.push('\n');
            }
        }
        ("application/x-ndjson", out)
    };

    let mut response = Response::new(Body::from(body));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

/// GET /api/diagnostics/snapshot
pub async fn diagnostics_snapshot_handler(
    State(state): State<WebState>,
    Query(query): Query<DiagnosticsQuery>,
) -> impl IntoResponse {
    let engine = state.engine();
    let app_state = engine.app_state();
    let source_names = default_log_sources()
        .into_iter()
        .filter(|source| source.path.exists())
        .map(|source| source.source.to_string())
        .collect::<Vec<_>>();

    Json(DiagnosticsSnapshot {
        profile_id: query.profile_id,
        timestamp: now_millis(),
        frontend_event_count: 0,
        ipc_event_count: 0,
        renderer_state_count: 0,
        captured_sse_count: 0,
        captured_ws_count: 0,
        enabled: true,
        events: Vec::new(),
        session_traces: Vec::new(),
        renderer_states: Vec::new(),
        metadata: json!({
            "cwd": engine.cwd(),
            "session_id": engine.current_session_id().to_string(),
            "model": app_state.main_loop_model,
            "backend": app_state.main_loop_backend,
            "log_sources": source_names,
        }),
    })
}

/// GET /api/diagnostics/traces
pub async fn diagnostics_traces_handler(Query(query): Query<TracesQuery>) -> impl IntoResponse {
    let logs_query = LogsQuery {
        source: None,
        level: None,
        category: None,
        session_id: query.session_id.clone(),
        limit: Some(MAX_LIMIT),
        ..LogsQuery::default()
    };
    let entries = filter_entries(collect_logs(&logs_query).entries, &logs_query);
    let (traces, detail) = build_traces(entries, &query);

    Json(TracesResponse {
        profile_id: query.profile_id,
        traces,
        detail,
    })
}

fn collect_logs(query: &LogsQuery) -> CollectedLogs {
    collect_logs_from_specs(log_sources_for_query(query))
}

fn collect_logs_from_specs(sources: Vec<LogSourceSpec>) -> CollectedLogs {
    let mut entries = Vec::new();
    let mut source_truncated = false;

    for spec in sources {
        let Ok((lines, truncated)) = read_newest_lines(&spec.path, MAX_LINES_PER_SOURCE) else {
            continue;
        };
        if lines.is_empty() {
            continue;
        }
        source_truncated |= truncated;

        for (line_index, line) in lines.into_iter().enumerate() {
            entries.push(parse_log_line(&line, spec.source, line_index));
        }
    }

    entries.sort_by(|a, b| {
        b.timestamp
            .cmp(&a.timestamp)
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.id.cmp(&b.id))
    });

    CollectedLogs {
        entries,
        source_truncated,
    }
}

fn default_log_sources() -> Vec<LogSourceSpec> {
    vec![
        LogSourceSpec {
            source: "backend",
            path: PathBuf::from(BACKEND_DEV_LOG),
        },
        LogSourceSpec {
            source: "frontend",
            path: PathBuf::from(FRONTEND_DEV_LOG),
        },
    ]
}

fn log_sources_for_query(query: &LogsQuery) -> Vec<LogSourceSpec> {
    match query
        .source
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some("backend") => vec![LogSourceSpec {
            source: "backend",
            path: PathBuf::from(BACKEND_DEV_LOG),
        }],
        Some("frontend") => vec![LogSourceSpec {
            source: "frontend",
            path: PathBuf::from(FRONTEND_DEV_LOG),
        }],
        Some(_) => default_log_sources(),
        None => default_log_sources(),
    }
}

fn read_newest_lines(path: &Path, max_lines: usize) -> std::io::Result<(Vec<String>, bool)> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut lines = VecDeque::with_capacity(max_lines);
    let mut truncated = false;

    for line in reader.lines() {
        let line = line?;
        if lines.len() == max_lines {
            lines.pop_front();
            truncated = true;
        }
        lines.push_back(line);
    }

    Ok((lines.into_iter().collect(), truncated))
}

fn parse_log_line(line: &str, default_source: &str, line_index: usize) -> LogEntry {
    let trimmed = line.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return parse_json_log(value, default_source, line_index);
    }

    let (timestamp, message) = split_rfc3339_prefix(trimmed)
        .map(|(ts, rest)| (ts, rest.trim().to_string()))
        .unwrap_or_else(|| (now_millis(), trimmed.to_string()));

    let level = detect_level(trimmed).unwrap_or_else(|| "info".to_string());
    let metadata = plain_text_metadata(trimmed);
    let session_id = metadata
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let turn_id = metadata
        .get("turn_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let trace_id = metadata
        .get("trace_id")
        .and_then(Value::as_str)
        .map(str::to_string);

    LogEntry {
        id: format!("{default_source}:{line_index}"),
        timestamp,
        level,
        source: default_source.to_string(),
        category: Some(infer_category(trimmed)),
        message,
        event_type: None,
        session_id,
        turn_id,
        trace_id,
        payload_summary: Some(trimmed.to_string()),
        payload_raw: None,
        metadata: (!metadata.is_empty()).then_some(Value::Object(metadata)),
    }
}

fn parse_json_log(value: Value, default_source: &str, line_index: usize) -> LogEntry {
    let obj = value.as_object();
    let timestamp = obj
        .and_then(|map| {
            first_value(map, &["timestamp", "time", "ts"]).and_then(parse_timestamp_value)
        })
        .unwrap_or_else(now_millis);
    let level = obj
        .and_then(|map| first_string(map, &["level", "severity"]))
        .map(normalize_level)
        .unwrap_or_else(|| "info".to_string());
    let source = obj
        .and_then(|map| first_string(map, &["source", "target"]))
        .unwrap_or(default_source)
        .to_string();
    let message = obj
        .and_then(|map| first_string(map, &["message", "msg", "event", "event_type"]))
        .unwrap_or_else(|| value.as_str().unwrap_or("json log entry"))
        .to_string();
    let category = obj
        .and_then(|map| first_string(map, &["category", "kind"]))
        .map(str::to_string)
        .or_else(|| Some(infer_category(&message)));
    let event_type = obj
        .and_then(|map| first_string(map, &["event_type", "type", "event"]))
        .map(str::to_string);
    let session_id = obj
        .and_then(|map| first_string(map, &["session_id", "session"]))
        .map(str::to_string);
    let turn_id = obj
        .and_then(|map| first_string(map, &["turn_id", "turn"]))
        .map(str::to_string);
    let trace_id = obj
        .and_then(|map| first_string(map, &["trace_id", "trace"]))
        .map(str::to_string);

    LogEntry {
        id: format!("{source}:{line_index}"),
        timestamp,
        level,
        source,
        category,
        message: message.clone(),
        event_type,
        session_id,
        turn_id,
        trace_id,
        payload_summary: Some(message),
        payload_raw: Some(value),
        metadata: None,
    }
}

fn first_value<'a>(map: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| map.get(*key))
}

fn first_string<'a>(map: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    first_value(map, keys).and_then(Value::as_str)
}

fn split_rfc3339_prefix(line: &str) -> Option<(i64, &str)> {
    let split_at = line
        .char_indices()
        .find_map(|(idx, ch)| ch.is_whitespace().then_some(idx))?;
    let (candidate, rest) = line.split_at(split_at);
    parse_timestamp_str(candidate).map(|millis| (millis, rest))
}

fn parse_timestamp_value(value: &Value) -> Option<i64> {
    if let Some(n) = value.as_i64() {
        return normalize_numeric_timestamp(n);
    }
    if let Some(n) = value.as_u64() {
        return normalize_numeric_timestamp(n as i64);
    }
    value.as_str().and_then(parse_timestamp_str)
}

fn parse_since(value: Option<&str>) -> Option<i64> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(n) = value.parse::<i64>() {
        return normalize_numeric_timestamp(n);
    }
    parse_timestamp_str(value)
}

fn parse_timestamp_str(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.timestamp_millis())
        .ok()
        .or_else(|| {
            DateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f %z")
                .map(|dt| dt.timestamp_millis())
                .ok()
        })
}

fn normalize_numeric_timestamp(value: i64) -> Option<i64> {
    if value <= 0 {
        return None;
    }
    if value < 10_000_000_000 {
        Some(value * 1000)
    } else {
        Some(value)
    }
}

fn now_millis() -> i64 {
    Utc::now().timestamp_millis()
}

fn detect_level(line: &str) -> Option<String> {
    let upper = line.to_ascii_uppercase();
    if upper.contains("ERROR") {
        Some("error".to_string())
    } else if upper.contains("WARN") {
        Some("warn".to_string())
    } else if upper.contains("DEBUG") {
        Some("debug".to_string())
    } else if upper.contains("INFO") {
        Some("info".to_string())
    } else {
        None
    }
}

fn normalize_level(level: &str) -> String {
    match level.trim().to_ascii_lowercase().as_str() {
        "warning" | "warn" => "warn".to_string(),
        "err" | "error" => "error".to_string(),
        "debug" | "trace" => "debug".to_string(),
        "info" | "notice" => "info".to_string(),
        other => other.to_string(),
    }
}

fn infer_category(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if lower.contains("diagnostic") || lower.contains("trace") {
        "diagnostics".to_string()
    } else if lower.contains("chat") || lower.contains("message") {
        "chat".to_string()
    } else if lower.contains("setting") || lower.contains("config") {
        "settings".to_string()
    } else if lower.contains("websocket") || lower.contains("ipc") || lower.contains("sse") {
        "diagnostics".to_string()
    } else {
        "log".to_string()
    }
}

fn plain_text_metadata(line: &str) -> Map<String, Value> {
    let mut metadata = Map::new();
    for key in ["session_id", "turn_id", "trace_id", "request_id"] {
        if let Some(value) = extract_text_field(line, key) {
            metadata.insert(key.to_string(), Value::String(value));
        }
    }
    metadata
}

fn extract_text_field(line: &str, key: &str) -> Option<String> {
    for marker in [format!("{key}="), format!("{key}:")] {
        if let Some(start) = line.find(marker.as_str()) {
            let value = &line[start + marker.len()..];
            let value = value
                .trim_start_matches(['"', '\''])
                .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';' || ch == '"')
                .next()
                .unwrap_or("")
                .trim_matches(['"', '\'']);
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

struct PageEntries {
    entries: Vec<LogEntry>,
    next_cursor: Option<String>,
    has_more: bool,
}

fn page_entries(entries: Vec<LogEntry>, query: &LogsQuery) -> PageEntries {
    let entries = filter_entries(entries, query);
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = parse_cursor(query.cursor.as_deref()).unwrap_or(0);
    let total = entries.len();
    let page = entries
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    let next_offset = offset + page.len();
    let has_more = next_offset < total;

    PageEntries {
        entries: page,
        next_cursor: has_more.then(|| format!("offset:{next_offset}")),
        has_more,
    }
}

fn parse_cursor(cursor: Option<&str>) -> Option<usize> {
    let cursor = cursor?.trim();
    cursor
        .strip_prefix("offset:")
        .unwrap_or(cursor)
        .parse::<usize>()
        .ok()
}

fn filter_entries(entries: Vec<LogEntry>, query: &LogsQuery) -> Vec<LogEntry> {
    let source = normalized_filter(query.source.as_deref());
    let level = normalized_filter(query.level.as_deref());
    let category = normalized_filter(query.category.as_deref());
    let session_id = query
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let since = parse_since(query.since.as_deref());
    let search = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);

    entries
        .into_iter()
        .filter(|entry| {
            if let Some(source) = source.as_deref() {
                if entry.source != source {
                    return false;
                }
            }
            if let Some(level) = level.as_deref() {
                if entry.level != level {
                    return false;
                }
            }
            if let Some(category) = category.as_deref() {
                if entry.category.as_deref() != Some(category) {
                    return false;
                }
            }
            if let Some(session_id) = session_id {
                if entry.session_id.as_deref() != Some(session_id) {
                    return false;
                }
            }
            if let Some(since) = since {
                if entry.timestamp < since {
                    return false;
                }
            }
            if let Some(search) = search.as_deref() {
                if !entry_matches_search(entry, search) {
                    return false;
                }
            }
            true
        })
        .collect()
}

fn normalized_filter(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty() && *v != "all")
        .map(str::to_ascii_lowercase)
}

fn entry_matches_search(entry: &LogEntry, search: &str) -> bool {
    [
        entry.id.as_str(),
        entry.source.as_str(),
        entry.level.as_str(),
        entry.category.as_deref().unwrap_or_default(),
        entry.event_type.as_deref().unwrap_or_default(),
        entry.message.as_str(),
        entry.payload_summary.as_deref().unwrap_or_default(),
        entry.session_id.as_deref().unwrap_or_default(),
        entry.turn_id.as_deref().unwrap_or_default(),
    ]
    .iter()
    .any(|value| value.to_ascii_lowercase().contains(search))
}

fn build_traces(
    entries: Vec<LogEntry>,
    query: &TracesQuery,
) -> (Vec<TraceSummary>, Option<TraceDetail>) {
    let mut groups: BTreeMap<(String, Option<String>), Vec<LogEntry>> = BTreeMap::new();

    for entry in entries {
        let Some(session_id) = entry.session_id.clone() else {
            continue;
        };
        if let Some(filter) = query.session_id.as_deref() {
            if session_id != filter {
                continue;
            }
        }
        if let Some(filter) = query.turn_id.as_deref() {
            if entry.turn_id.as_deref() != Some(filter) {
                continue;
            }
        }
        groups
            .entry((session_id, entry.turn_id.clone()))
            .or_default()
            .push(entry);
    }

    let mut traces = Vec::new();
    let mut detail = None;
    for ((session_id, turn_id), group) in groups {
        let summary = trace_summary(session_id, turn_id, &group);
        if query
            .turn_id
            .as_deref()
            .is_some_and(|filter| summary.turn_id.as_deref() == Some(filter))
        {
            let events = group
                .iter()
                .map(diagnostics_event_from_log_entry)
                .collect::<Vec<_>>();
            detail = Some(trace_detail(summary.clone(), &events));
        }
        traces.push(summary);
    }
    traces.sort_by(|a, b| b.started_at.cmp(&a.started_at));

    (traces, detail)
}

fn trace_summary(session_id: String, turn_id: Option<String>, group: &[LogEntry]) -> TraceSummary {
    let started_at = group.iter().map(|entry| entry.timestamp).min();
    let completed_at = group.iter().map(|entry| entry.timestamp).max();
    let error = group
        .iter()
        .find(|entry| entry.level == "error")
        .map(|entry| entry.message.clone());
    let status = if error.is_some() {
        Some("error".to_string())
    } else {
        Some("unknown".to_string())
    };
    let id = match &turn_id {
        Some(turn_id) => format!("{session_id}:{turn_id}"),
        None => session_id.clone(),
    };

    TraceSummary {
        id,
        session_id,
        turn_id,
        status,
        started_at,
        completed_at,
        event_count: group.len(),
        error,
    }
}

fn trace_detail(summary: TraceSummary, events: &[DiagnosticsEvent]) -> TraceDetail {
    TraceDetail {
        id: summary.id,
        session_id: summary.session_id,
        turn_id: summary.turn_id,
        status: summary.status,
        started_at: summary.started_at,
        completed_at: summary.completed_at,
        event_count: summary.event_count,
        error: summary.error,
        events: events.to_vec(),
        turns: Vec::new(),
        metadata: json!({ "source": "logs" }),
    }
}

fn diagnostics_event_from_log_entry(entry: &LogEntry) -> DiagnosticsEvent {
    DiagnosticsEvent {
        timestamp: entry.timestamp,
        source: entry.source.clone(),
        category: entry.category.clone(),
        session_id: entry.session_id.clone().unwrap_or_default(),
        turn_id: entry.turn_id.clone(),
        message_id: None,
        event_type: entry
            .event_type
            .clone()
            .unwrap_or_else(|| entry.category.clone().unwrap_or_else(|| "log".to_string())),
        payload_summary: entry
            .payload_summary
            .clone()
            .unwrap_or_else(|| entry.message.clone()),
        payload_raw: entry.payload_raw.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::WebState;
    use allthecodes_engine::lifecycle::QueryEngine;
    use allthecodes_engine::types::config::QueryEngineConfig;
    use axum::body::to_bytes;
    use axum::response::IntoResponse;
    use serde_json::{json, Value};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    async fn response_json(response: axum::response::Response) -> Value {
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("response body");
        serde_json::from_slice(&body).expect("json body")
    }

    fn make_web_state() -> WebState {
        let engine = Arc::new(QueryEngine::new(QueryEngineConfig {
            cwd: ".".to_string(),
            tools: vec![],
            custom_system_prompt: None,
            append_system_prompt: None,
            user_specified_model: None,
            fallback_model: None,
            max_turns: None,
            max_budget_usd: None,
            task_budget: None,
            verbose: false,
            initial_messages: None,
            commands: vec![],
            thinking_config: None,
            json_schema: None,
            replay_user_messages: false,
            persist_session: false,
            resolved_model: None,
            auto_save_session: false,
            agent_context: None,
        }));
        WebState::new(engine, Arc::new(AtomicBool::new(false)))
    }

    #[tokio::test]
    async fn diagnostics_snapshot_returns_empty_snapshot_without_trace_store() {
        let response = diagnostics_snapshot_handler(
            State(make_web_state()),
            Query(DiagnosticsQuery {
                profile_id: Some("profile-a".to_string()),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["profile_id"], json!("profile-a"));
        assert_eq!(body["frontend_event_count"], json!(0));
        assert_eq!(body["ipc_event_count"], json!(0));
        assert_eq!(body["renderer_state_count"], json!(0));
        assert_eq!(body["captured_sse_count"], json!(0));
        assert_eq!(body["captured_ws_count"], json!(0));
        assert_eq!(body["events"], json!([]));
        assert_eq!(body["session_traces"], json!([]));
    }

    #[tokio::test]
    async fn diagnostics_traces_returns_empty_list_without_trace_store() {
        let response = diagnostics_traces_handler(Query(TracesQuery::default()))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["traces"], json!([]));
        assert_eq!(body["detail"], Value::Null);
    }

    #[test]
    fn logs_handler_parses_plain_text_backend_log() {
        let entry = parse_log_line(
            "2026-06-05T10:11:12Z WARN session_id=s1 turn_id=t1 backend restarted",
            "backend",
            7,
        );

        assert_eq!(entry.source, "backend");
        assert_eq!(entry.level, "warn");
        assert_eq!(entry.timestamp, 1_780_654_272_000);
        assert_eq!(
            entry.message,
            "WARN session_id=s1 turn_id=t1 backend restarted"
        );
        assert_eq!(entry.session_id.as_deref(), Some("s1"));
        assert_eq!(entry.turn_id.as_deref(), Some("t1"));
    }

    #[test]
    fn logs_handler_filters_by_level_and_search() {
        let entries = vec![
            parse_log_line("2026-06-05T10:00:00Z INFO alpha", "backend", 0),
            parse_log_line("2026-06-05T10:00:01Z ERROR beta target", "backend", 1),
            parse_log_line("2026-06-05T10:00:02Z ERROR gamma target", "backend", 2),
        ];
        let query = LogsQuery {
            level: Some("error".to_string()),
            search: Some("gamma".to_string()),
            limit: Some(1),
            ..LogsQuery::default()
        };

        let page = page_entries(entries, &query);

        assert_eq!(page.entries.len(), 1);
        assert!(page.entries[0].message.contains("gamma"));
        assert!(!page.has_more);
    }

    #[tokio::test]
    async fn logs_export_jsonl_uses_same_filters() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("backend.log");
        std::fs::write(
            &path,
            "2026-06-05T10:00:00Z INFO alpha\n2026-06-05T10:00:01Z ERROR beta\n",
        )
        .expect("write log");
        let collected = collect_logs_from_specs(vec![LogSourceSpec {
            source: "backend",
            path,
        }]);
        let query = LogsQuery {
            level: Some("error".to_string()),
            ..LogsQuery::default()
        };
        let entries = filter_entries(collected.entries, &query);
        let body = entries
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()
            .expect("json lines")
            .join("\n");

        assert!(body.contains("beta"));
        assert!(!body.contains("alpha"));
    }

    #[test]
    fn capabilities_marks_logs_ready() {
        let caps = crate::handlers::capabilities_map();
        assert_eq!(caps.get("logs"), Some(&true));
    }
}
