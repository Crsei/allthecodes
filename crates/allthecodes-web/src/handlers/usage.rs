//! Usage dashboard API handler.
//!
//! GET /api/usage?period=24h|7d|30d|90d|all&profile_id=
//!
//! Aggregates persisted assistant-message usage from `{data_root}/sessions`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Datelike, Duration, SecondsFormat, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state::WebState;
use allthecodes_protocol::ApiError as ProtocolApiError;
use allthecodes_session::request_snapshot::ApiRequestSnapshot;
use allthecodes_session::storage::SessionFile;
use allthecodes_types::message::Usage;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct UsageDashboardResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub period: String,
    pub generated_at: u64,
    pub totals: UsageTotalsResponse,
    pub buckets: Vec<UsageBucket>,
    pub by_model: Vec<UsageModelRow>,
    pub by_provider: Vec<UsageProviderRow>,
    pub partial: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct UsageTotalsResponse {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub total_cost_usd: f64,
    pub api_call_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_count: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct UsageBucket {
    pub start: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub api_call_count: u64,
    pub cost_usd: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_count: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct UsageModelRow {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub usage: UsageTotalsResponse,
}

#[derive(Debug, Serialize)]
pub struct UsageProviderRow {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub usage: UsageTotalsResponse,
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct UsageQuery {
    pub period: Option<String>,
    pub profile_id: Option<String>,
}

const VALID_PERIODS: &[&str] = &["24h", "7d", "30d", "90d", "all"];

/// Validate and normalize a period string.
pub fn normalize_period(raw: &str) -> Result<String, Response> {
    let lower = raw.trim().to_lowercase();
    if VALID_PERIODS.contains(&lower.as_str()) {
        Ok(lower)
    } else {
        let valid = VALID_PERIODS.join(", ");
        let body = ProtocolApiError::BadRequest {
            code: "invalid_period",
            message: format!("invalid period '{}'. Must be one of: {}", raw, valid),
        }
        .into_body();
        Err((StatusCode::BAD_REQUEST, Json(body)).into_response())
    }
}

#[derive(Debug, Clone, Copy)]
enum BucketKind {
    Hour,
    Day,
    Month,
}

#[derive(Debug, Clone, Copy)]
struct PeriodSpec {
    since: Option<i64>,
    bucket_kind: BucketKind,
}

#[derive(Debug, Clone)]
struct UsageEvent {
    session_id: String,
    timestamp: i64,
    usage: Usage,
    cost_usd: f64,
    request: Option<RequestIdentity>,
}

#[derive(Debug, Clone)]
struct RequestIdentity {
    provider: String,
    model: String,
}

#[derive(Debug, Default)]
struct UsageTotalsAccumulator {
    total_input_tokens: u64,
    total_output_tokens: u64,
    total_cache_read_tokens: u64,
    total_cache_creation_tokens: u64,
    total_cost_usd: f64,
    api_call_count: u64,
    session_ids: BTreeSet<String>,
}

impl UsageTotalsAccumulator {
    fn add_event(&mut self, event: &UsageEvent) {
        self.total_input_tokens = self
            .total_input_tokens
            .saturating_add(event.usage.input_tokens);
        self.total_output_tokens = self
            .total_output_tokens
            .saturating_add(event.usage.output_tokens);
        self.total_cache_read_tokens = self
            .total_cache_read_tokens
            .saturating_add(event.usage.cache_read_input_tokens);
        self.total_cache_creation_tokens = self
            .total_cache_creation_tokens
            .saturating_add(event.usage.cache_creation_input_tokens);
        self.total_cost_usd += event.cost_usd;
        self.api_call_count = self.api_call_count.saturating_add(1);
        self.session_ids.insert(event.session_id.clone());
    }

    fn to_response(&self) -> UsageTotalsResponse {
        UsageTotalsResponse {
            total_input_tokens: self.total_input_tokens,
            total_output_tokens: self.total_output_tokens,
            total_cache_read_tokens: self.total_cache_read_tokens,
            total_cache_creation_tokens: self.total_cache_creation_tokens,
            total_cost_usd: self.total_cost_usd,
            api_call_count: self.api_call_count,
            session_count: Some(self.session_ids.len() as u64),
        }
    }
}

#[derive(Debug)]
struct BreakdownAccumulator {
    id: String,
    label: String,
    provider_id: Option<String>,
    provider_id_is_mixed: bool,
    model_id: Option<String>,
    usage: UsageTotalsAccumulator,
}

impl BreakdownAccumulator {
    fn add_event(&mut self, event: &UsageEvent) {
        self.usage.add_event(event);
    }

    fn note_model_provider(&mut self, provider_id: Option<&str>) {
        if self.provider_id_is_mixed {
            return;
        }

        let Some(provider_id) = provider_id else {
            return;
        };

        match self.provider_id.as_deref() {
            None => self.provider_id = Some(provider_id.to_string()),
            Some(existing) if existing == provider_id => {}
            Some(_) => {
                self.provider_id = None;
                self.provider_id_is_mixed = true;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// GET /api/usage
pub async fn usage_handler(
    State(_state): State<WebState>,
    Query(query): Query<UsageQuery>,
) -> Result<Json<UsageDashboardResponse>, Response> {
    let period = match query.period.as_deref() {
        Some(raw) => normalize_period(raw)?,
        None => "7d".to_string(),
    };

    let generated_at = current_unix_seconds();
    Ok(Json(aggregate_usage_dashboard(
        period,
        query.profile_id,
        generated_at,
    )))
}

fn aggregate_usage_dashboard(
    period: String,
    profile_id: Option<String>,
    generated_at: u64,
) -> UsageDashboardResponse {
    let generated_at_i64 = generated_at.min(i64::MAX as u64) as i64;
    let spec = period_spec(&period, generated_at_i64);
    let sessions_dir = allthecodes_config::paths::sessions_dir();
    let mut warnings = Vec::new();
    let mut skipped_session_files = 0_u64;

    let mut totals = UsageTotalsAccumulator::default();
    let mut buckets: BTreeMap<i64, UsageTotalsAccumulator> = BTreeMap::new();
    let mut by_model: BTreeMap<String, BreakdownAccumulator> = BTreeMap::new();
    let mut by_provider: BTreeMap<String, BreakdownAccumulator> = BTreeMap::new();

    if sessions_dir.exists() {
        match std::fs::read_dir(&sessions_dir) {
            Ok(entries) => {
                for entry in entries {
                    let Ok(entry) = entry else {
                        skipped_session_files = skipped_session_files.saturating_add(1);
                        continue;
                    };
                    let path = entry.path();
                    if !is_session_json_candidate(&path) {
                        continue;
                    }

                    let events = match read_session_usage_events(&path) {
                        Ok(events) => events,
                        Err(_) => {
                            skipped_session_files = skipped_session_files.saturating_add(1);
                            continue;
                        }
                    };

                    for event in events {
                        if !event_in_period(event.timestamp, generated_at_i64, spec.since) {
                            continue;
                        }

                        totals.add_event(&event);

                        if let Some(bucket_start) =
                            bucket_start_seconds(spec.bucket_kind, event.timestamp)
                        {
                            buckets.entry(bucket_start).or_default().add_event(&event);
                        }

                        add_model_breakdown(&mut by_model, &event);
                        add_provider_breakdown(&mut by_provider, &event);
                    }
                }
            }
            Err(error) => {
                warnings.push(format!("Failed to read usage session directory: {}", error))
            }
        }
    }

    if skipped_session_files > 0 {
        warnings.push(format!(
            "Skipped {} unreadable session files.",
            skipped_session_files
        ));
    }

    UsageDashboardResponse {
        profile_id,
        period,
        generated_at,
        totals: totals.to_response(),
        buckets: buckets_to_response(buckets, spec.bucket_kind),
        by_model: model_rows_to_response(by_model),
        by_provider: provider_rows_to_response(by_provider),
        partial: false,
        warnings,
    }
}

fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn period_spec(period: &str, generated_at: i64) -> PeriodSpec {
    match period {
        "24h" => PeriodSpec {
            since: generated_at.checked_sub(24 * 60 * 60),
            bucket_kind: BucketKind::Hour,
        },
        "7d" => PeriodSpec {
            since: generated_at.checked_sub(7 * 24 * 60 * 60),
            bucket_kind: BucketKind::Day,
        },
        "30d" => PeriodSpec {
            since: generated_at.checked_sub(30 * 24 * 60 * 60),
            bucket_kind: BucketKind::Day,
        },
        "90d" => PeriodSpec {
            since: generated_at.checked_sub(90 * 24 * 60 * 60),
            bucket_kind: BucketKind::Day,
        },
        _ => PeriodSpec {
            since: None,
            bucket_kind: BucketKind::Month,
        },
    }
}

fn event_in_period(timestamp: i64, generated_at: i64, since: Option<i64>) -> bool {
    match since {
        Some(since) => timestamp >= since && timestamp <= generated_at,
        None => true,
    }
}

fn is_session_json_candidate(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    if path.extension().is_none_or(|ext| ext != "json") {
        return false;
    }

    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    !file_name.contains(".rewind-") && !file_name.contains(".archived-")
}

fn read_session_usage_events(path: &Path) -> Result<Vec<UsageEvent>, ()> {
    let contents = std::fs::read_to_string(path).map_err(|_| ())?;
    let session_file: SessionFile = serde_json::from_str(&contents).map_err(|_| ())?;
    let mut events = usage_events_from_session(&session_file);
    attach_request_identities(&session_file.session_id, &mut events);
    Ok(events)
}

fn usage_events_from_session(session_file: &SessionFile) -> Vec<UsageEvent> {
    session_file
        .messages
        .iter()
        .filter(|message| message.msg_type == "assistant")
        .filter_map(|message| {
            let usage = message.data.get("usage")?;
            if usage.is_null() {
                return None;
            }

            let usage: Usage = serde_json::from_value(usage.clone()).ok()?;
            Some(UsageEvent {
                session_id: session_file.session_id.clone(),
                timestamp: normalize_timestamp_seconds(message.timestamp),
                usage,
                cost_usd: value_as_f64(message.data.get("cost_usd")),
                request: None,
            })
        })
        .collect()
}

fn attach_request_identities(session_id: &str, events: &mut [UsageEvent]) {
    if events.is_empty() {
        return;
    }

    let Ok(mut snapshots) =
        allthecodes_session::request_snapshot::load_api_request_snapshots(session_id)
    else {
        return;
    };

    if snapshots.len() != events.len() {
        return;
    }

    snapshots.sort_by_key(|snapshot| snapshot.sequence);
    for (event, snapshot) in events.iter_mut().zip(snapshots) {
        event.request = request_identity_from_snapshot(snapshot);
    }
}

fn request_identity_from_snapshot(snapshot: ApiRequestSnapshot) -> Option<RequestIdentity> {
    let provider = snapshot.provider.trim();
    let model = snapshot.model.trim();
    if provider.is_empty() || model.is_empty() {
        return None;
    }

    Some(RequestIdentity {
        provider: provider.to_string(),
        model: model.to_string(),
    })
}

fn value_as_f64(value: Option<&Value>) -> f64 {
    value.and_then(Value::as_f64).unwrap_or(0.0)
}

fn normalize_timestamp_seconds(timestamp: i64) -> i64 {
    if timestamp.unsigned_abs() >= 10_000_000_000 {
        timestamp / 1_000
    } else {
        timestamp
    }
}

fn add_model_breakdown(by_model: &mut BTreeMap<String, BreakdownAccumulator>, event: &UsageEvent) {
    let model_id = event.request.as_ref().map(|request| request.model.as_str());
    let provider_id = event
        .request
        .as_ref()
        .map(|request| request.provider.as_str());
    let key = model_id.unwrap_or("unknown").to_string();
    let entry = by_model
        .entry(key.clone())
        .or_insert_with(|| match model_id {
            Some(model_id) => BreakdownAccumulator {
                id: key,
                label: model_id.to_string(),
                provider_id: provider_id.map(ToOwned::to_owned),
                provider_id_is_mixed: false,
                model_id: Some(model_id.to_string()),
                usage: UsageTotalsAccumulator::default(),
            },
            None => BreakdownAccumulator {
                id: "unknown".to_string(),
                label: "Unknown model".to_string(),
                provider_id: None,
                provider_id_is_mixed: false,
                model_id: None,
                usage: UsageTotalsAccumulator::default(),
            },
        });
    entry.note_model_provider(provider_id);
    entry.add_event(event);
}

fn add_provider_breakdown(
    by_provider: &mut BTreeMap<String, BreakdownAccumulator>,
    event: &UsageEvent,
) {
    let provider_id = event
        .request
        .as_ref()
        .map(|request| request.provider.as_str());
    let key = provider_id.unwrap_or("unknown").to_string();
    let entry = by_provider
        .entry(key.clone())
        .or_insert_with(|| match provider_id {
            Some(provider_id) => BreakdownAccumulator {
                id: key,
                label: provider_id.to_string(),
                provider_id: Some(provider_id.to_string()),
                provider_id_is_mixed: false,
                model_id: None,
                usage: UsageTotalsAccumulator::default(),
            },
            None => BreakdownAccumulator {
                id: "unknown".to_string(),
                label: "Unknown provider".to_string(),
                provider_id: None,
                provider_id_is_mixed: false,
                model_id: None,
                usage: UsageTotalsAccumulator::default(),
            },
        });
    entry.add_event(event);
}

fn buckets_to_response(
    buckets: BTreeMap<i64, UsageTotalsAccumulator>,
    bucket_kind: BucketKind,
) -> Vec<UsageBucket> {
    buckets
        .into_iter()
        .filter_map(|(start_seconds, usage)| {
            let start = timestamp_to_utc(start_seconds)?;
            let end = bucket_end(start, bucket_kind);
            Some(UsageBucket {
                start: format_datetime(start),
                end: end.map(format_datetime),
                label: Some(bucket_label(start, bucket_kind)),
                input_tokens: usage.total_input_tokens,
                output_tokens: usage.total_output_tokens,
                cache_read_tokens: usage.total_cache_read_tokens,
                cache_creation_tokens: usage.total_cache_creation_tokens,
                api_call_count: usage.api_call_count,
                cost_usd: usage.total_cost_usd,
                session_count: Some(usage.session_ids.len() as u64),
            })
        })
        .collect()
}

fn model_rows_to_response(by_model: BTreeMap<String, BreakdownAccumulator>) -> Vec<UsageModelRow> {
    let mut rows: Vec<UsageModelRow> = by_model
        .into_values()
        .map(|row| UsageModelRow {
            id: row.id,
            label: row.label,
            provider_id: row.provider_id,
            model_id: row.model_id,
            usage: row.usage.to_response(),
        })
        .collect();
    sort_model_rows(&mut rows);
    rows
}

fn provider_rows_to_response(
    by_provider: BTreeMap<String, BreakdownAccumulator>,
) -> Vec<UsageProviderRow> {
    let mut rows: Vec<UsageProviderRow> = by_provider
        .into_values()
        .map(|row| UsageProviderRow {
            id: row.id,
            label: row.label,
            provider_id: row.provider_id,
            model_id: row.model_id,
            usage: row.usage.to_response(),
        })
        .collect();
    sort_provider_rows(&mut rows);
    rows
}

fn sort_model_rows(rows: &mut [UsageModelRow]) {
    rows.sort_by(|a, b| {
        usage_sort_key(&b.usage)
            .cmp(&usage_sort_key(&a.usage))
            .then_with(|| a.label.cmp(&b.label))
    });
}

fn sort_provider_rows(rows: &mut [UsageProviderRow]) {
    rows.sort_by(|a, b| {
        usage_sort_key(&b.usage)
            .cmp(&usage_sort_key(&a.usage))
            .then_with(|| a.label.cmp(&b.label))
    });
}

fn usage_sort_key(usage: &UsageTotalsResponse) -> u64 {
    usage
        .total_input_tokens
        .saturating_add(usage.total_output_tokens)
        .saturating_add(usage.total_cache_read_tokens)
        .saturating_add(usage.total_cache_creation_tokens)
}

fn bucket_start_seconds(kind: BucketKind, timestamp: i64) -> Option<i64> {
    let dt = timestamp_to_utc(timestamp)?;
    let start = match kind {
        BucketKind::Hour => dt.with_minute(0)?.with_second(0)?.with_nanosecond(0)?,
        BucketKind::Day => Utc
            .with_ymd_and_hms(dt.year(), dt.month(), dt.day(), 0, 0, 0)
            .single()?,
        BucketKind::Month => Utc
            .with_ymd_and_hms(dt.year(), dt.month(), 1, 0, 0, 0)
            .single()?,
    };
    Some(start.timestamp())
}

fn bucket_end(start: DateTime<Utc>, kind: BucketKind) -> Option<DateTime<Utc>> {
    match kind {
        BucketKind::Hour => start.checked_add_signed(Duration::hours(1)),
        BucketKind::Day => start.checked_add_signed(Duration::days(1)),
        BucketKind::Month => {
            let (year, month) = if start.month() == 12 {
                (start.year().checked_add(1)?, 1)
            } else {
                (start.year(), start.month() + 1)
            };
            Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0).single()
        }
    }
}

fn bucket_label(start: DateTime<Utc>, kind: BucketKind) -> String {
    match kind {
        BucketKind::Hour => start.format("%Y-%m-%d %H:00").to_string(),
        BucketKind::Day => start.format("%Y-%m-%d").to_string(),
        BucketKind::Month => start.format("%Y-%m").to_string(),
    }
}

fn timestamp_to_utc(timestamp: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_opt(timestamp, 0).single()
}

fn format_datetime(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{Query, State};
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use serde_json::json;
    use serial_test::serial;
    use tempfile::TempDir;

    use crate::handlers::test_support::{make_web_state, response_json};

    struct HomeGuard {
        previous: Option<String>,
    }

    impl HomeGuard {
        fn set(path: &Path) -> Self {
            let previous = std::env::var("ALLTHECODES_HOME").ok();
            std::env::set_var("ALLTHECODES_HOME", path);
            Self { previous }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var("ALLTHECODES_HOME", previous);
            } else {
                std::env::remove_var("ALLTHECODES_HOME");
            }
        }
    }

    fn temp_home() -> (TempDir, HomeGuard) {
        let home = tempfile::tempdir().expect("tempdir");
        let guard = HomeGuard::set(home.path());
        (home, guard)
    }

    fn write_session(session_id: &str, messages: Vec<Value>) {
        let dir = allthecodes_config::paths::sessions_dir();
        std::fs::create_dir_all(&dir).expect("sessions dir");
        let session = json!({
            "session_id": session_id,
            "created_at": 1_700_000_000,
            "last_modified": 1_700_000_000,
            "cwd": "/tmp/project",
            "messages": messages,
        });
        std::fs::write(
            dir.join(format!("{session_id}.json")),
            serde_json::to_vec_pretty(&session).expect("session json"),
        )
        .expect("write session");
    }

    fn write_request_snapshots(session_id: &str, snapshots: Vec<Value>) {
        let dir = allthecodes_config::paths::sessions_dir();
        std::fs::create_dir_all(&dir).expect("sessions dir");
        let mut body = String::new();
        for snapshot in snapshots {
            body.push_str(&serde_json::to_string(&snapshot).expect("snapshot json"));
            body.push('\n');
        }
        std::fs::write(dir.join(format!("{session_id}.requests.ndjson")), body)
            .expect("write snapshots");
    }

    fn assistant_usage(uuid: &str, timestamp: i64, usage: Value, cost_usd: f64) -> Value {
        json!({
            "type": "assistant",
            "uuid": uuid,
            "timestamp": timestamp,
            "data": {
                "content": [],
                "stop_reason": "end_turn",
                "usage": usage,
                "cost_usd": cost_usd,
            }
        })
    }

    fn user_message(uuid: &str, timestamp: i64) -> Value {
        json!({
            "type": "user",
            "uuid": uuid,
            "timestamp": timestamp,
            "data": {
                "content": "hello",
                "is_meta": false,
            }
        })
    }

    fn request_snapshot(sequence: usize, session_id: &str, provider: &str, model: &str) -> Value {
        json!({
            "schema_version": 1,
            "captured_at": "2026-01-01T00:00:00Z",
            "session_id": session_id,
            "request_id": format!("req-{sequence}"),
            "sequence": sequence,
            "provider": provider,
            "model": model,
            "message_count": 1,
            "system_count": 0,
            "tool_count": 0,
            "max_tokens": 1000,
            "stream": true,
            "request": { "model": model, "messages": [] },
        })
    }

    fn usage(input: u64, output: u64, cache_read: u64, cache_creation: u64) -> Value {
        json!({
            "input_tokens": input,
            "output_tokens": output,
            "cache_read_input_tokens": cache_read,
            "cache_creation_input_tokens": cache_creation,
        })
    }

    #[tokio::test]
    #[serial]
    async fn handler_empty_sessions_dir_returns_zero_dashboard() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();

        let response = usage_handler(
            State(state),
            Query(UsageQuery {
                period: None,
                profile_id: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["period"], json!("7d"));
        assert_eq!(body["totals"]["total_input_tokens"], json!(0));
        assert_eq!(body["totals"]["api_call_count"], json!(0));
        assert_eq!(body["totals"]["session_count"], json!(0));
        assert_eq!(body["partial"], json!(false));
        assert!(body["warnings"].is_null());
    }

    #[test]
    #[serial]
    fn single_session_assistant_usage_aggregates_totals() {
        let (_home, _guard) = temp_home();
        let now = 1_800_000_000;
        write_session(
            "session-1",
            vec![
                user_message("u1", now - 100),
                assistant_usage("a1", now - 50, usage(100, 50, 20, 10), 0.125),
            ],
        );

        let dashboard = aggregate_usage_dashboard("7d".to_string(), None, now as u64);

        assert_eq!(dashboard.totals.total_input_tokens, 100);
        assert_eq!(dashboard.totals.total_output_tokens, 50);
        assert_eq!(dashboard.totals.total_cache_read_tokens, 20);
        assert_eq!(dashboard.totals.total_cache_creation_tokens, 10);
        assert_eq!(dashboard.totals.api_call_count, 1);
        assert_eq!(dashboard.totals.session_count, Some(1));
        assert!((dashboard.totals.total_cost_usd - 0.125).abs() < f64::EPSILON);
        assert!(!dashboard.partial);
    }

    #[test]
    #[serial]
    fn period_filter_accepts_seconds_and_milliseconds() {
        let (_home, _guard) = temp_home();
        let now = 1_800_000_000;
        write_session(
            "session-seconds",
            vec![assistant_usage("a1", now - 60, usage(10, 1, 0, 0), 0.01)],
        );
        write_session(
            "session-millis",
            vec![assistant_usage(
                "a2",
                (now - 120) * 1_000,
                usage(20, 2, 0, 0),
                0.02,
            )],
        );
        write_session(
            "session-old",
            vec![assistant_usage(
                "a3",
                now - (8 * 24 * 60 * 60),
                usage(30, 3, 0, 0),
                0.03,
            )],
        );

        let dashboard = aggregate_usage_dashboard("7d".to_string(), None, now as u64);

        assert_eq!(dashboard.totals.total_input_tokens, 30);
        assert_eq!(dashboard.totals.total_output_tokens, 3);
        assert_eq!(dashboard.totals.api_call_count, 2);
        assert_eq!(dashboard.totals.session_count, Some(2));
    }

    #[test]
    #[serial]
    fn buckets_are_grouped_by_period_granularity() {
        let (_home, _guard) = temp_home();
        let day_a = Utc
            .with_ymd_and_hms(2027, 1, 10, 1, 0, 0)
            .single()
            .expect("date")
            .timestamp();
        let day_b = Utc
            .with_ymd_and_hms(2027, 1, 11, 1, 0, 0)
            .single()
            .expect("date")
            .timestamp();
        let now = day_b + 3_600;
        write_session(
            "session-buckets",
            vec![
                assistant_usage("a1", day_a, usage(10, 0, 0, 0), 0.0),
                assistant_usage("a2", day_a + 3600, usage(20, 0, 0, 0), 0.0),
                assistant_usage("a3", day_b, usage(30, 0, 0, 0), 0.0),
            ],
        );

        let daily = aggregate_usage_dashboard("7d".to_string(), None, now as u64);
        assert_eq!(daily.buckets.len(), 2);
        assert_eq!(daily.buckets[0].label.as_deref(), Some("2027-01-10"));
        assert_eq!(daily.buckets[0].input_tokens, 30);
        assert_eq!(daily.buckets[1].label.as_deref(), Some("2027-01-11"));
        assert_eq!(daily.buckets[1].input_tokens, 30);

        let hourly = aggregate_usage_dashboard("24h".to_string(), None, now as u64);
        assert!(hourly
            .buckets
            .iter()
            .any(|bucket| bucket.label.as_deref() == Some("2027-01-11 01:00")));

        let monthly = aggregate_usage_dashboard("all".to_string(), None, now as u64);
        assert_eq!(monthly.buckets.len(), 1);
        assert_eq!(monthly.buckets[0].label.as_deref(), Some("2027-01"));
        assert_eq!(monthly.buckets[0].input_tokens, 60);
    }

    #[test]
    #[serial]
    fn request_snapshots_populate_model_and_provider_breakdowns() {
        let (_home, _guard) = temp_home();
        let now = 1_800_000_000;
        write_session(
            "session-breakdown",
            vec![
                assistant_usage("a1", now - 10, usage(10, 1, 0, 0), 0.01),
                assistant_usage("a2", now - 5, usage(20, 2, 0, 0), 0.02),
            ],
        );
        write_request_snapshots(
            "session-breakdown",
            vec![
                request_snapshot(0, "session-breakdown", "anthropic", "claude-sonnet"),
                request_snapshot(1, "session-breakdown", "openai", "gpt-4.1"),
            ],
        );

        let dashboard = aggregate_usage_dashboard("7d".to_string(), None, now as u64);

        assert_eq!(dashboard.by_model.len(), 2);
        assert_eq!(dashboard.by_model[0].id, "gpt-4.1");
        assert_eq!(dashboard.by_model[0].usage.total_input_tokens, 20);
        assert_eq!(dashboard.by_model[1].id, "claude-sonnet");
        assert_eq!(dashboard.by_provider.len(), 2);
        assert_eq!(dashboard.by_provider[0].id, "openai");
        assert_eq!(dashboard.by_provider[0].usage.total_input_tokens, 20);
        assert_eq!(dashboard.by_provider[1].id, "anthropic");
    }

    #[test]
    #[serial]
    fn missing_or_mismatched_snapshots_are_grouped_as_unknown() {
        let (_home, _guard) = temp_home();
        let now = 1_800_000_000;
        write_session(
            "session-mismatch",
            vec![
                assistant_usage("a1", now - 10, usage(10, 1, 0, 0), 0.01),
                assistant_usage("a2", now - 5, usage(20, 2, 0, 0), 0.02),
            ],
        );
        write_request_snapshots(
            "session-mismatch",
            vec![request_snapshot(
                0,
                "session-mismatch",
                "anthropic",
                "claude-sonnet",
            )],
        );

        let dashboard = aggregate_usage_dashboard("7d".to_string(), None, now as u64);

        assert_eq!(dashboard.by_model.len(), 1);
        assert_eq!(dashboard.by_model[0].id, "unknown");
        assert_eq!(dashboard.by_model[0].label, "Unknown model");
        assert_eq!(dashboard.by_model[0].usage.total_input_tokens, 30);
        assert_eq!(dashboard.by_provider.len(), 1);
        assert_eq!(dashboard.by_provider[0].id, "unknown");
        assert_eq!(dashboard.by_provider[0].label, "Unknown provider");
    }

    #[test]
    #[serial]
    fn unreadable_session_files_are_skipped_with_warning() {
        let (_home, _guard) = temp_home();
        let now = 1_800_000_000;
        write_session(
            "session-good",
            vec![assistant_usage("a1", now - 10, usage(10, 1, 0, 0), 0.01)],
        );
        let dir = allthecodes_config::paths::sessions_dir();
        std::fs::write(dir.join("bad.json"), b"{ not json").expect("bad session");

        let dashboard = aggregate_usage_dashboard("7d".to_string(), None, now as u64);

        assert_eq!(dashboard.totals.total_input_tokens, 10);
        assert_eq!(
            dashboard.warnings,
            vec!["Skipped 1 unreadable session files.".to_string()]
        );
    }
}

#[cfg(test)]
#[path = "usage_api_tests.rs"]
mod api_tests;
