//! Shared cost ledger: load, backfill, and aggregate cost events.
//!
//! All user-visible cost/usage aggregations should use this module
//! for consistent semantics across `/cost`, `/extra-usage`, `/insights`,
//! statusline, and Web usage dashboard.
//!
//! # Data Sources
//!
//! The cost ledger prefers runtime audit events from `events.ndjson` (written
//! by the observability subsystem). When those are not available (e.g. sessions
//! created before the audit system was active), it falls back to backfilling
//! from the [`AssistantMessage`] structs in the session file.
//!
//! # Usage
//!
//! ```ignore
//! // Quick one-shot summary for a session:
//! let summary = get_session_cost_summary(session_id, &messages);
//!
//! // Loading raw events for programmatic use:
//! let events = get_session_cost_events(session_id, &messages);
//!
//! // Aggregation over a custom set of events:
//! let summary = aggregate_cost_events(&events);
//! ```

use serde::{Deserialize, Serialize};
use tracing::warn;

use allthecodes_types::message::Message;
use allthecodes_types::models::PricingSource;

// ---------------------------------------------------------------------------
// CostEvent: single atomic cost record
// ---------------------------------------------------------------------------

/// A single cost event, either from a runtime audit event or backfilled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEvent {
    pub schema_version: u32,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
    pub submit_id: Option<String>,
    pub turn_id: Option<String>,
    pub request_id: Option<String>,
    pub message_id: Option<String>,
    pub provider: Option<String>,
    pub backend: Option<String>,
    pub model: Option<String>,
    pub pricing: Option<CostEventPricing>,
    pub usage: CostEventUsage,
    pub cost_usd: f64,
    pub stop_reason: Option<String>,
    pub is_retry: bool,
    pub attempt: u32,
    pub backfilled: bool,
}

/// Pricing snapshot captured at cost-recording time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEventPricing {
    pub source: PricingSource,
    pub matched_key: String,
    pub currency: String,
    pub input_per_1m: f64,
    pub output_per_1m: f64,
    pub cache_read_multiplier: f64,
    pub cache_creation_multiplier: f64,
}

/// Token-usage snapshot for a single API call.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostEventUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_output_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
}

impl CostEventUsage {
    /// Total tokens consumed (input + output + all cache).
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens
            + self.output_tokens
            + self.cache_read_input_tokens
            + self.cache_creation_input_tokens
    }
}

// ---------------------------------------------------------------------------
// CostSummary: aggregated result
// ---------------------------------------------------------------------------

/// Aggregated cost summary for display.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostSummary {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub total_reasoning_output_tokens: u64,
    pub total_cost_usd: f64,
    pub api_calls: u64,
    pub unknown_pricing_count: u64,
    pub backfilled_count: u64,

    /// Per-model breakdown.
    pub by_model: Vec<ModelCostSummary>,
}

/// Per-model cost/token bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCostSummary {
    pub model: String,
    pub api_calls: u64,
    pub total_tokens: u64,
    pub total_cost_usd: f64,
}

// ---------------------------------------------------------------------------
// Runtime-event loading (events.ndjson)
// ---------------------------------------------------------------------------

/// Load cost events for a session by reading the runtime `events.ndjson` file.
///
/// Returns an empty `Vec` when:
/// - The runs directory does not exist.
/// - The `events.ndjson` file does not exist.
/// - No cost-recorded events are found in the file.
pub fn load_session_cost_events(session_id: &str) -> Vec<CostEvent> {
    let runs_dir = allthecodes_config::paths::runs_dir(session_id);
    let events_path = runs_dir.join("events.ndjson");

    if !events_path.exists() {
        return Vec::new();
    }

    let contents = match std::fs::read_to_string(&events_path) {
        Ok(c) => c,
        Err(e) => {
            warn!(
                "Failed to read events.ndjson for session {session_id}: {e}",
                session_id = session_id,
                e = e
            );
            return Vec::new();
        }
    };

    let mut events = Vec::new();

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(event) = try_parse_cost_line(line) {
            events.push(event);
        }
    }

    events
}

/// Try to find and parse the cost-event payload in a single NDJSON line.
///
/// Returns `None` if the line is not a cost-recorded audit event or cannot
/// be parsed.
fn try_parse_cost_line(line: &str) -> Option<CostEvent> {
    let audit_event: serde_json::Value = serde_json::from_str(line).ok()?;
    let kind = audit_event.get("kind")?.as_str()?;
    if kind != "cost_recorded" {
        return None;
    }
    let data = audit_event.get("data")?;
    let mut event = serde_json::from_value::<CostEvent>(data.clone())
        .ok()
        .or_else(|| parse_flat_cost_event(&audit_event, data))?;
    fill_event_from_audit_envelope(&mut event, &audit_event);
    Some(event)
}

fn fill_event_from_audit_envelope(event: &mut CostEvent, audit_event: &serde_json::Value) {
    if event.timestamp.is_none() {
        event.timestamp = audit_event
            .get("ts")
            .and_then(serde_json::Value::as_str)
            .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
            .map(|ts| ts.timestamp());
    }
    if event.session_id.is_empty() {
        if let Some(session_id) = value_string(audit_event, "session_id") {
            event.session_id = session_id;
        }
    }
    if event.submit_id.is_none() {
        event.submit_id = value_string(audit_event, "submit_id");
    }
    if event.turn_id.is_none() {
        event.turn_id = value_string(audit_event, "turn_id");
    }
    if event.request_id.is_none() {
        event.request_id = value_string(audit_event, "request_id");
    }
    if event.message_id.is_none() {
        event.message_id = value_string(audit_event, "message_id");
    }
}

fn parse_flat_cost_event(
    audit_event: &serde_json::Value,
    data: &serde_json::Value,
) -> Option<CostEvent> {
    let model = value_string(data, "model");
    let pricing_source = data
        .get("pricing_source")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or(PricingSource::Unknown);
    let matched_key = value_string(data, "pricing_matched_key").unwrap_or_default();

    Some(CostEvent {
        schema_version: value_u64(data, "schema_version").unwrap_or(1) as u32,
        session_id: value_string(audit_event, "session_id").unwrap_or_default(),
        timestamp: None,
        submit_id: None,
        turn_id: None,
        request_id: None,
        message_id: value_string(data, "message_id"),
        provider: value_string(data, "provider"),
        backend: value_string(data, "backend"),
        model,
        pricing: Some(CostEventPricing {
            source: pricing_source,
            matched_key,
            currency: "USD".into(),
            input_per_1m: 0.0,
            output_per_1m: 0.0,
            cache_read_multiplier: 0.1,
            cache_creation_multiplier: 1.25,
        }),
        usage: CostEventUsage {
            input_tokens: value_u64(data, "input_tokens").unwrap_or_default(),
            output_tokens: value_u64(data, "output_tokens").unwrap_or_default(),
            reasoning_output_tokens: value_u64(data, "reasoning_output_tokens").unwrap_or_default(),
            cache_read_input_tokens: value_u64(data, "cache_read_input_tokens").unwrap_or_default(),
            cache_creation_input_tokens: value_u64(data, "cache_creation_input_tokens")
                .unwrap_or_default(),
        },
        cost_usd: value_f64(data, "cost_usd").unwrap_or_default(),
        stop_reason: value_string(data, "stop_reason"),
        is_retry: data
            .get("is_retry")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        attempt: value_u64(data, "attempt").unwrap_or(1) as u32,
        backfilled: data
            .get("backfilled")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    })
}

fn value_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

fn value_u64(value: &serde_json::Value, key: &str) -> Option<u64> {
    value.get(key).and_then(serde_json::Value::as_u64)
}

fn value_f64(value: &serde_json::Value, key: &str) -> Option<f64> {
    value.get(key).and_then(serde_json::Value::as_f64)
}

fn normalize_timestamp_seconds(timestamp: i64) -> i64 {
    if timestamp.unsigned_abs() >= 10_000_000_000 {
        timestamp / 1_000
    } else {
        timestamp
    }
}

// ---------------------------------------------------------------------------
// Backfill from messages
// ---------------------------------------------------------------------------

/// Backfill cost events from session assistant messages.
///
/// Each [`Message::Assistant`] with non-zero usage or cost produces one
/// [`CostEvent`] with `backfilled: true`. Messages that are API errors or
/// have zero usage are skipped.
pub fn backfill_cost_events_from_messages(
    session_id: &str,
    messages: &[Message],
) -> Vec<CostEvent> {
    let mut events = Vec::new();

    for msg in messages {
        let Message::Assistant(a) = msg else {
            continue;
        };

        // Skip API error messages; they did not incur real usage.
        if a.is_api_error_message {
            continue;
        }

        // Skip messages without any usage data.
        let Some(usage) = &a.usage else {
            continue;
        };
        if usage.input_tokens == 0
            && usage.output_tokens == 0
            && usage.cache_read_input_tokens == 0
            && usage.cache_creation_input_tokens == 0
        {
            continue;
        }

        let model = None; // Not available on AssistantMessage currently.
        let pricing_info = Some(CostEventPricing {
            source: PricingSource::Backfilled,
            matched_key: String::new(),
            currency: "USD".into(),
            input_per_1m: 0.0,
            output_per_1m: 0.0,
            cache_read_multiplier: 0.1,
            cache_creation_multiplier: 1.25,
        });

        events.push(CostEvent {
            schema_version: 1,
            session_id: session_id.to_string(),
            timestamp: Some(normalize_timestamp_seconds(a.timestamp)),
            submit_id: None,
            turn_id: None,
            request_id: None,
            message_id: Some(a.uuid.to_string()),
            provider: None,
            backend: None,
            model,
            pricing: pricing_info,
            usage: CostEventUsage {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                reasoning_output_tokens: usage.reasoning_output_tokens,
                cache_read_input_tokens: usage.cache_read_input_tokens,
                cache_creation_input_tokens: usage.cache_creation_input_tokens,
            },
            cost_usd: a.cost_usd,
            stop_reason: a.stop_reason.clone(),
            is_retry: false,
            attempt: 0,
            backfilled: true,
        });
    }

    events
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

/// Aggregate a list of cost events into a summary.
///
/// Counts total tokens and cost across all events, tracking per-model
/// breakdowns, unknown-pricing events, and backfilled events.
pub fn aggregate_cost_events(events: &[CostEvent]) -> CostSummary {
    use std::collections::BTreeMap;

    let mut total_input: u64 = 0;
    let mut total_output: u64 = 0;
    let mut total_cache_read: u64 = 0;
    let mut total_cache_creation: u64 = 0;
    let mut total_reasoning: u64 = 0;
    let mut total_cost: f64 = 0.0;
    let mut api_calls: u64 = 0;
    let mut unknown_count: u64 = 0;
    let mut backfilled_count: u64 = 0;

    // Per-model buckets: model_name -> (api_calls, total_tokens, total_cost_usd)
    let mut by_model: BTreeMap<String, (u64, u64, f64)> = BTreeMap::new();

    for event in events {
        api_calls += 1;

        total_input = total_input.saturating_add(event.usage.input_tokens);
        total_output = total_output.saturating_add(event.usage.output_tokens);
        total_cache_read = total_cache_read.saturating_add(event.usage.cache_read_input_tokens);
        total_cache_creation =
            total_cache_creation.saturating_add(event.usage.cache_creation_input_tokens);
        total_reasoning = total_reasoning.saturating_add(event.usage.reasoning_output_tokens);
        total_cost += event.cost_usd;

        if event.backfilled {
            backfilled_count += 1;
        }

        // Determine the model label for per-model breakdown.
        let model_label = event
            .pricing
            .as_ref()
            .map(|p| {
                if p.matched_key.is_empty() {
                    event.model.clone().unwrap_or_else(|| "unknown".into())
                } else {
                    p.matched_key.clone()
                }
            })
            .or_else(|| event.model.clone())
            .unwrap_or_else(|| "unknown".into());

        // Track unknown pricing.
        if let Some(ref pricing) = event.pricing {
            if pricing.source == PricingSource::Unknown
                || (pricing.matched_key.is_empty() && pricing.source != PricingSource::Backfilled)
            {
                unknown_count += 1;
            }
        } else {
            unknown_count += 1;
        }

        let entry = by_model.entry(model_label).or_insert((0, 0, 0.0));
        entry.0 += 1;
        entry.1 += event.usage.total_tokens();
        entry.2 += event.cost_usd;
    }

    let by_model: Vec<ModelCostSummary> = by_model
        .into_iter()
        .map(|(model, (calls, tokens, cost))| ModelCostSummary {
            model,
            api_calls: calls,
            total_tokens: tokens,
            total_cost_usd: cost,
        })
        .collect();

    CostSummary {
        total_input_tokens: total_input,
        total_output_tokens: total_output,
        total_cache_read_tokens: total_cache_read,
        total_cache_creation_tokens: total_cache_creation,
        total_reasoning_output_tokens: total_reasoning,
        total_cost_usd: total_cost,
        api_calls,
        unknown_pricing_count: unknown_count,
        backfilled_count,
        by_model,
    }
}

// ---------------------------------------------------------------------------
// Session-level convenience
// ---------------------------------------------------------------------------

/// Get cost events for a session: prefer runtime events, fall back to backfill.
///
/// This is the primary entry point for session cost lookups. It first attempts
/// to load from the runtime `events.ndjson` file. If no runtime events are
/// found, it backfills from the provided session messages.
pub fn get_session_cost_events(session_id: &str, messages: &[Message]) -> Vec<CostEvent> {
    let runtime_events = load_session_cost_events(session_id);

    if !runtime_events.is_empty() {
        return runtime_events;
    }

    backfill_cost_events_from_messages(session_id, messages)
}

/// Convenience: load cost events and aggregate in one call.
pub fn get_session_cost_summary(session_id: &str, messages: &[Message]) -> CostSummary {
    let events = get_session_cost_events(session_id, messages);
    aggregate_cost_events(&events)
}

/// Get cost events for a session, falling back to loading the session JSON
/// file from disk and backfilling from assistant messages when runtime
/// events are not available.
///
/// Unlike [`get_session_cost_events`], this function **does not** require
/// in-memory messages.  It loads the session file on demand, so it is
/// suited for contexts (Web API, export pipelines) where the caller does
/// not hold the conversation in memory.
///
/// This function is **read-only**: it never rewrites the session file.
pub fn get_session_cost_events_with_backfill_from_json(session_id: &str) -> Vec<CostEvent> {
    let runtime_events = load_session_cost_events(session_id);

    if !runtime_events.is_empty() {
        return runtime_events;
    }

    // No runtime events; try loading the session JSON file.
    let messages = match allthecodes_session::storage::load_session(session_id) {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };

    backfill_cost_events_from_messages(session_id, &messages)
}

/// Convenience: load cost events (with JSON backfill) and aggregate in one call.
pub fn get_session_cost_summary_from_json(session_id: &str) -> CostSummary {
    let events = get_session_cost_events_with_backfill_from_json(session_id);
    aggregate_cost_events(&events)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{AssistantMessage, Usage};
    use allthecodes_types::models::pricing;
    use uuid::Uuid;

    fn make_event(
        model: Option<&str>,
        input: u64,
        output: u64,
        cache_read: u64,
        cache_create: u64,
        reasoning: u64,
        cost: f64,
        backfilled: bool,
        pricing_source: Option<PricingSource>,
    ) -> CostEvent {
        let pricing = model.map(|m| {
            let pm = pricing::get_pricing_match(m);
            CostEventPricing {
                source: pricing_source.unwrap_or(pm.source),
                matched_key: pm.matched_key,
                currency: "USD".into(),
                input_per_1m: pm.pricing.input_per_1m,
                output_per_1m: pm.pricing.output_per_1m,
                cache_read_multiplier: 0.1,
                cache_creation_multiplier: 1.25,
            }
        });

        CostEvent {
            schema_version: 1,
            session_id: "test-session".into(),
            timestamp: Some(1_800_000_000),
            submit_id: None,
            turn_id: None,
            request_id: None,
            message_id: None,
            provider: None,
            backend: None,
            model: model.map(|m| m.to_string()),
            pricing,
            usage: CostEventUsage {
                input_tokens: input,
                output_tokens: output,
                reasoning_output_tokens: reasoning,
                cache_read_input_tokens: cache_read,
                cache_creation_input_tokens: cache_create,
            },
            cost_usd: cost,
            stop_reason: Some("end_turn".into()),
            is_retry: false,
            attempt: 0,
            backfilled,
        }
    }

    fn make_backfilled_event(
        model: Option<&str>,
        input: u64,
        output: u64,
        cache_read: u64,
        cache_create: u64,
        reasoning: u64,
        cost: f64,
    ) -> CostEvent {
        make_event(
            model,
            input,
            output,
            cache_read,
            cache_create,
            reasoning,
            cost,
            true,
            None,
        )
    }

    fn make_assistant_msg(
        input_tokens: u64,
        output_tokens: u64,
        cache_read: u64,
        cache_create: u64,
        reasoning: u64,
        cost: f64,
    ) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: Vec::new(),
            usage: Some(Usage {
                input_tokens,
                output_tokens,
                reasoning_output_tokens: reasoning,
                cache_read_input_tokens: cache_read,
                cache_creation_input_tokens: cache_create,
            }),
            stop_reason: Some("end_turn".into()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: cost,
        })
    }

    fn make_assistant_msg_no_usage() -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: Vec::new(),
            usage: None,
            stop_reason: Some("end_turn".into()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
    }

    // Empty events list

    #[test]
    fn aggregate_empty_events() {
        let summary = aggregate_cost_events(&[]);
        assert_eq!(summary.total_input_tokens, 0);
        assert_eq!(summary.total_output_tokens, 0);
        assert_eq!(summary.total_cost_usd, 0.0);
        assert_eq!(summary.api_calls, 0);
        assert_eq!(summary.unknown_pricing_count, 0);
        assert_eq!(summary.backfilled_count, 0);
        assert!(summary.by_model.is_empty());
    }

    #[test]
    fn backfill_empty_messages() {
        let events = backfill_cost_events_from_messages("test-session", &[]);
        assert!(events.is_empty());
    }

    // Single event aggregation

    #[test]
    fn aggregate_single_event() {
        let events = vec![make_event(
            Some("gpt-4o"),
            100,
            50,
            20,
            10,
            5,
            0.0015,
            false,
            None,
        )];
        let summary = aggregate_cost_events(&events);

        assert_eq!(summary.total_input_tokens, 100);
        assert_eq!(summary.total_output_tokens, 50);
        assert_eq!(summary.total_cache_read_tokens, 20);
        assert_eq!(summary.total_cache_creation_tokens, 10);
        assert_eq!(summary.total_reasoning_output_tokens, 5);
        assert!((summary.total_cost_usd - 0.0015).abs() < 1e-12);
        assert_eq!(summary.api_calls, 1);
        assert_eq!(summary.unknown_pricing_count, 0);
        assert_eq!(summary.backfilled_count, 0);
        assert_eq!(summary.by_model.len(), 1);
        assert_eq!(summary.by_model[0].model, "gpt-4o");
        assert_eq!(summary.by_model[0].api_calls, 1);
        assert_eq!(summary.by_model[0].total_tokens, 180); // 100+50+20+10
    }

    #[test]
    fn aggregate_single_event_unknown_pricing() {
        let events = vec![make_event(
            Some("unknown-model"),
            100,
            50,
            0,
            0,
            0,
            0.0,
            false,
            Some(PricingSource::Unknown),
        )];
        let summary = aggregate_cost_events(&events);
        assert_eq!(summary.unknown_pricing_count, 1);
        assert_eq!(summary.api_calls, 1);
    }

    #[test]
    fn aggregate_single_event_no_pricing() {
        let mut event = make_event(Some("gpt-4o"), 100, 50, 0, 0, 0, 0.001, false, None);
        event.pricing = None;
        let summary = aggregate_cost_events(&[event]);
        assert_eq!(summary.unknown_pricing_count, 1);
        assert_eq!(summary.api_calls, 1);
    }

    // Multiple events with different models

    #[test]
    fn aggregate_multiple_events_different_models() {
        let events = vec![
            make_event(Some("gpt-4o"), 100, 50, 0, 0, 0, 0.001, false, None),
            make_event(
                Some("claude-sonnet-4"),
                200,
                100,
                50,
                25,
                10,
                0.005,
                false,
                None,
            ),
            make_event(Some("gpt-4o"), 50, 30, 10, 5, 0, 0.0008, false, None),
        ];

        let summary = aggregate_cost_events(&events);

        assert_eq!(summary.total_input_tokens, 350);
        assert_eq!(summary.total_output_tokens, 180);
        assert_eq!(summary.total_cache_read_tokens, 60);
        assert_eq!(summary.total_cache_creation_tokens, 30);
        assert_eq!(summary.total_reasoning_output_tokens, 10);
        assert!((summary.total_cost_usd - 0.0068).abs() < 1e-12);
        assert_eq!(summary.api_calls, 3);
        assert_eq!(summary.unknown_pricing_count, 0);

        // Two model buckets, sorted alphabetically.
        assert_eq!(summary.by_model.len(), 2);
        // BTreeMap order: "claude-sonnet-4" < "gpt-4o".
        assert_eq!(summary.by_model[0].model, "claude-sonnet-4");
        assert_eq!(summary.by_model[0].api_calls, 1);
        assert_eq!(summary.by_model[1].model, "gpt-4o");
        assert_eq!(summary.by_model[1].api_calls, 2);
    }

    #[test]
    fn aggregate_multiple_events_three_models() {
        let events = vec![
            make_event(Some("gpt-4o-mini"), 50, 30, 0, 0, 0, 0.0002, false, None),
            make_event(Some("gpt-4o"), 100, 50, 0, 0, 0, 0.001, false, None),
            make_event(
                Some("claude-sonnet-4"),
                200,
                100,
                0,
                0,
                0,
                0.003,
                false,
                None,
            ),
        ];

        let summary = aggregate_cost_events(&events);

        assert_eq!(summary.by_model.len(), 3);
        assert_eq!(summary.by_model[0].model, "claude-sonnet-4");
        assert_eq!(summary.by_model[1].model, "gpt-4o");
        assert_eq!(summary.by_model[2].model, "gpt-4o-mini");
    }

    // Backfilled event markers

    #[test]
    fn backfilled_events_are_counted() {
        let events = vec![
            make_backfilled_event(Some("gpt-4o"), 100, 50, 0, 0, 0, 0.001),
            make_backfilled_event(Some("gpt-4o"), 200, 100, 0, 0, 0, 0.002),
            make_event(
                Some("claude-sonnet-4"),
                300,
                150,
                0,
                0,
                0,
                0.004,
                false,
                None,
            ),
        ];

        let summary = aggregate_cost_events(&events);
        assert_eq!(summary.backfilled_count, 2);
        assert_eq!(summary.api_calls, 3);
    }

    #[test]
    fn backfill_from_messages_sets_backfilled_flag() {
        let messages = vec![
            make_assistant_msg(100, 50, 20, 10, 5, 0.001),
            make_assistant_msg(200, 100, 0, 0, 0, 0.002),
        ];

        let events = backfill_cost_events_from_messages("test-session", &messages);
        assert_eq!(events.len(), 2);
        for event in &events {
            assert!(event.backfilled);
            assert_eq!(event.session_id, "test-session");
            assert!(event.message_id.is_some());
        }
    }

    #[test]
    fn backfill_skips_api_error_messages() {
        let error_msg = Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: Vec::new(),
            usage: Some(Usage {
                input_tokens: 100,
                output_tokens: 50,
                reasoning_output_tokens: 0,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            }),
            stop_reason: Some("end_turn".into()),
            is_api_error_message: true,
            api_error: Some("rate_limit".into()),
            cost_usd: 0.0,
        });

        let events = backfill_cost_events_from_messages("test-session", &[error_msg]);
        assert!(events.is_empty());
    }

    #[test]
    fn backfill_skips_messages_without_usage() {
        let messages = vec![make_assistant_msg_no_usage()];
        let events = backfill_cost_events_from_messages("test-session", &messages);
        assert!(events.is_empty());
    }

    #[test]
    fn backfill_skips_zero_usage_messages() {
        let messages = vec![make_assistant_msg(0, 0, 0, 0, 0, 0.0)];
        let events = backfill_cost_events_from_messages("test-session", &messages);
        assert!(events.is_empty());
    }

    // Unknown pricing counting

    #[test]
    fn unknown_pricing_from_empty_matched_key() {
        let mut event = make_event(
            Some("weird-model"),
            100,
            50,
            0,
            0,
            0,
            0.001,
            false,
            Some(PricingSource::Builtin),
        );
        // Empty matched_key should be counted as unknown.
        if let Some(ref mut pricing) = event.pricing {
            pricing.matched_key.clear();
        }

        let summary = aggregate_cost_events(&[event]);
        assert_eq!(summary.unknown_pricing_count, 1);
    }

    #[test]
    fn unknown_pricing_mixed_with_known() {
        let events = vec![
            make_event(Some("gpt-4o"), 100, 50, 0, 0, 0, 0.001, false, None),
            make_event(
                Some("unknown-xyz"),
                50,
                30,
                0,
                0,
                0,
                0.0,
                false,
                Some(PricingSource::Unknown),
            ),
            make_event(Some("gpt-4o-mini"), 200, 100, 0, 0, 0, 0.002, false, None),
        ];

        let summary = aggregate_cost_events(&events);
        assert_eq!(summary.api_calls, 3);
        assert_eq!(summary.unknown_pricing_count, 1);
        // Known model events still contribute to totals.
        assert_eq!(summary.total_input_tokens, 350);
    }

    // Edge cases

    #[test]
    fn aggregate_large_token_counts_no_overflow() {
        let events = vec![make_event(
            Some("gpt-4o"),
            u64::MAX / 4,
            u64::MAX / 4,
            u64::MAX / 4,
            u64::MAX / 4,
            0,
            999.0,
            false,
            None,
        )];
        let summary = aggregate_cost_events(&events);
        // All four categories fit without overflow.
        assert_eq!(summary.total_input_tokens, u64::MAX / 4);
        assert_eq!(summary.total_output_tokens, u64::MAX / 4);
        assert_eq!(summary.total_cache_read_tokens, u64::MAX / 4);
        assert_eq!(summary.total_cache_creation_tokens, u64::MAX / 4);
    }

    #[test]
    fn cost_event_usage_total_tokens() {
        let usage = CostEventUsage {
            input_tokens: 100,
            output_tokens: 50,
            reasoning_output_tokens: 10,
            cache_read_input_tokens: 20,
            cache_creation_input_tokens: 5,
        };
        assert_eq!(usage.total_tokens(), 175);
    }

    #[test]
    fn cost_event_usage_total_tokens_zero() {
        let usage = CostEventUsage::default();
        assert_eq!(usage.total_tokens(), 0);
    }

    // Session-level convenience

    #[test]
    fn get_session_cost_summary_with_only_backfill() {
        let messages = vec![
            make_assistant_msg(100, 50, 0, 0, 0, 0.001),
            make_assistant_msg(200, 100, 50, 25, 10, 0.003),
        ];

        let summary = get_session_cost_summary("test-session", &messages);
        assert_eq!(summary.api_calls, 2);
        assert_eq!(summary.backfilled_count, 2);
        // Runtime events won't be found (no file), so all are backfilled.
        assert_eq!(summary.total_input_tokens, 300);
        assert_eq!(summary.total_output_tokens, 150);
        assert!((summary.total_cost_usd - 0.004).abs() < 1e-12);
    }

    #[test]
    fn get_session_cost_events_prefers_runtime_over_backfill() {
        // When no runtime events.ndjson file exists, should fall back to backfill.
        let messages = vec![make_assistant_msg(100, 50, 0, 0, 0, 0.001)];
        let events = get_session_cost_events("nonexistent-session", &messages);
        assert!(!events.is_empty());
        assert!(events.iter().all(|e| e.backfilled));
    }

    // Serialization round-trips

    #[test]
    fn cost_event_serde_roundtrip() {
        let event = make_event(
            Some("gpt-4o"),
            100,
            50,
            20,
            10,
            5,
            0.0015,
            false,
            Some(PricingSource::Builtin),
        );
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: CostEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.model.unwrap(), "gpt-4o");
        assert_eq!(deserialized.usage.input_tokens, 100);
        assert_eq!(deserialized.usage.output_tokens, 50);
        assert_eq!(deserialized.usage.cache_read_input_tokens, 20);
        assert_eq!(deserialized.usage.cache_creation_input_tokens, 10);
        assert_eq!(deserialized.usage.reasoning_output_tokens, 5);
        assert!((deserialized.cost_usd - 0.0015).abs() < 1e-12);
        assert!(deserialized.backfilled == false);
    }

    #[test]
    fn cost_event_usage_serde_roundtrip() {
        let usage = CostEventUsage {
            input_tokens: 1000,
            output_tokens: 500,
            reasoning_output_tokens: 50,
            cache_read_input_tokens: 200,
            cache_creation_input_tokens: 100,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let deserialized: CostEventUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.input_tokens, 1000);
        assert_eq!(deserialized.output_tokens, 500);
        assert_eq!(deserialized.reasoning_output_tokens, 50);
        assert_eq!(deserialized.cache_read_input_tokens, 200);
        assert_eq!(deserialized.cache_creation_input_tokens, 100);
    }

    #[test]
    fn cost_summary_serde_roundtrip() {
        let summary = CostSummary {
            total_input_tokens: 1000,
            total_output_tokens: 500,
            total_cache_read_tokens: 200,
            total_cache_creation_tokens: 100,
            total_reasoning_output_tokens: 50,
            total_cost_usd: 0.025,
            api_calls: 3,
            unknown_pricing_count: 0,
            backfilled_count: 1,
            by_model: vec![ModelCostSummary {
                model: "gpt-4o".into(),
                api_calls: 2,
                total_tokens: 1200,
                total_cost_usd: 0.02,
            }],
        };
        let json = serde_json::to_string(&summary).unwrap();
        let deserialized: CostSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.total_input_tokens, 1000);
        assert_eq!(deserialized.total_output_tokens, 500);
        assert_eq!(deserialized.api_calls, 3);
        assert_eq!(deserialized.by_model.len(), 1);
        assert_eq!(deserialized.by_model[0].model, "gpt-4o");
    }

    #[test]
    fn try_parse_cost_line_invalid_json() {
        assert!(try_parse_cost_line("not json").is_none());
    }

    #[test]
    fn try_parse_cost_line_wrong_kind() {
        let line = r#"{"kind":"session_start","data":{}}"#;
        assert!(try_parse_cost_line(line).is_none());
    }

    #[test]
    fn try_parse_cost_line_missing_data() {
        let line = r#"{"kind":"cost_recorded"}"#;
        assert!(try_parse_cost_line(line).is_none());
    }

    #[test]
    fn try_parse_cost_line_reads_nested_cost_event_and_envelope_ids() {
        let line = serde_json::json!({
            "event_id": "evt_cost",
            "ts": "2026-07-02T00:00:00Z",
            "session_id": "session-from-envelope",
            "submit_id": "sub_1",
            "turn_id": "turn_1",
            "request_id": "req_1",
            "message_id": "msg_1",
            "source": "tui",
            "kind": "cost_recorded",
            "stage": "cost",
            "level": "info",
            "outcome": "completed",
            "data": {
                "schema_version": 1,
                "session_id": "session-from-data",
                "message_id": "msg_data",
                "provider": "anthropic",
                "backend": "native",
                "model": "claude-sonnet-4-20250514",
                "pricing": {
                    "source": "builtin",
                    "matched_key": "claude-sonnet-4",
                    "currency": "USD",
                    "input_per_1m": 3.0,
                    "output_per_1m": 15.0,
                    "cache_read_multiplier": 0.1,
                    "cache_creation_multiplier": 1.25
                },
                "usage": {
                    "input_tokens": 100,
                    "output_tokens": 50,
                    "reasoning_output_tokens": 7,
                    "cache_read_input_tokens": 3,
                    "cache_creation_input_tokens": 2
                },
                "cost_usd": 0.001,
                "is_retry": false,
                "attempt": 1,
                "backfilled": false
            }
        })
        .to_string();

        let event = try_parse_cost_line(&line).expect("parse cost line");

        assert_eq!(event.session_id, "session-from-data");
        let expected_ts = chrono::DateTime::parse_from_rfc3339("2026-07-02T00:00:00Z")
            .unwrap()
            .timestamp();
        assert_eq!(event.timestamp, Some(expected_ts));
        assert_eq!(event.submit_id.as_deref(), Some("sub_1"));
        assert_eq!(event.turn_id.as_deref(), Some("turn_1"));
        assert_eq!(event.request_id.as_deref(), Some("req_1"));
        assert_eq!(event.message_id.as_deref(), Some("msg_data"));
        assert_eq!(event.usage.reasoning_output_tokens, 7);
        assert_eq!(
            event.pricing.as_ref().map(|pricing| pricing.source),
            Some(PricingSource::Builtin)
        );
    }

    // -----------------------------------------------------------------------
    // GAP FILL TESTS
    // -----------------------------------------------------------------------

    #[test]
    fn cost_event_cache_multipliers_present_in_pricing_struct() {
        // Phase 0 gap: verify that cache_read_multiplier = 0.1 and
        // cache_creation_multiplier = 1.25 are carried through the event.
        let event = make_event(Some("gpt-4o"), 100, 50, 20, 10, 5, 0.0015, false, None);

        let pricing = event.pricing.expect("pricing should be present");
        assert!(
            (pricing.cache_read_multiplier - 0.1).abs() < f64::EPSILON,
            "cache_read_multiplier should be 0.1, got {}",
            pricing.cache_read_multiplier
        );
        assert!(
            (pricing.cache_creation_multiplier - 1.25).abs() < f64::EPSILON,
            "cache_creation_multiplier should be 1.25, got {}",
            pricing.cache_creation_multiplier
        );
    }

    #[test]
    fn pricing_source_env_override_aggregation() {
        // Create a CostEvent with pricing.source = EnvOverride and verify
        // it is aggregated correctly (not counted as unknown pricing).
        let mut event = make_event(
            Some("gpt-4o"),
            100,
            50,
            0,
            0,
            0,
            0.001,
            false,
            Some(PricingSource::Builtin),
        );
        // Override to EnvOverride
        if let Some(ref mut pricing) = event.pricing {
            pricing.source = PricingSource::EnvOverride;
            pricing.matched_key = "__env_override__".into();
        }

        let summary = aggregate_cost_events(&[event]);

        assert_eq!(
            summary.unknown_pricing_count, 0,
            "EnvOverride events should NOT count as unknown pricing"
        );
        assert_eq!(summary.api_calls, 1);
        assert_eq!(summary.total_input_tokens, 100);
        assert!((summary.total_cost_usd - 0.001).abs() < 1e-12);
    }

    #[test]
    fn reasoning_output_tokens_counted_in_total_not_priced_separately() {
        // reasoning_output_tokens should be reflected in total_tokens()
        // (which feeds per-model total_tokens) but have no dedicated
        // aggregation counter of their own -- the existing
        // total_reasoning_output_tokens exists, but the cost_usd
        // for reasoning tokens is rolled into output_tokens costing.
        let event = make_event(
            Some("gpt-4o"),
            100,   // input
            50,    // output
            20,    // cache_read
            10,    // cache_create
            30,    // reasoning
            0.002, // cost (includes reasoning in output cost)
            false,
            None,
        );

        // total_tokens excludes reasoning_output_tokens
        assert_eq!(
            event.usage.total_tokens(),
            100 + 50 + 20 + 10,
            "total_tokens() sums input + output + cache_read + cache_create"
        );

        let summary = aggregate_cost_events(&[event.clone()]);

        // Reasoning is tracked separately in the summary
        assert_eq!(summary.total_reasoning_output_tokens, 30);
        // total_tokens for the model bucket is usage.total_tokens() which
        // excludes reasoning
        assert_eq!(summary.by_model[0].total_tokens, 180);
        // The cost_usd is passed through from the event -- aggregation
        // does NOT recompute it from pricing+usage.  The point is that
        // reasoning_output_tokens are not separately priced: they are
        // included in the output_tokens count that the provider charges,
        // so the single cost_usd covers both output and reasoning tokens.
        assert!(
            (summary.total_cost_usd - event.cost_usd).abs() < f64::EPSILON,
            "total_cost_usd should match the event's cost_usd"
        );
        // And there is no separate reasoning_cost field on CostSummary
        // -- only total_cost_usd.
        assert_eq!(
            summary.total_output_tokens, 50,
            "output tokens do NOT include reasoning"
        );
    }

    #[test]
    fn aggregate_mixed_known_unknown_backfilled_tracks_counts() {
        // Mix of known, unknown, and backfilled events -- verify counts
        // are tracked correctly for all three categories.
        let events = vec![
            // Known event (gpt-4o is in the pricing table)
            make_event(Some("gpt-4o"), 100, 50, 0, 0, 0, 0.001, false, None),
            // Unknown model -> unknown pricing
            make_event(
                Some("absolutely-unknown-model-v99"),
                50,
                25,
                0,
                0,
                0,
                0.0,
                false,
                Some(PricingSource::Unknown),
            ),
            // Backfilled event
            make_backfilled_event(Some("claude-sonnet-4"), 200, 100, 50, 25, 10, 0.005),
            // Another known event
            make_event(Some("gpt-4o-mini"), 200, 100, 0, 0, 0, 0.002, false, None),
        ];

        let summary = aggregate_cost_events(&events);

        assert_eq!(summary.api_calls, 4);
        assert_eq!(
            summary.unknown_pricing_count, 1,
            "only the absolutely-unknown-model should be unknown"
        );
        assert_eq!(
            summary.backfilled_count, 1,
            "only the backfilled event"
        );
        assert_eq!(summary.by_model.len(), 4, "four distinct model keys");
        // Totals should include everything
        assert_eq!(summary.total_input_tokens, 100 + 50 + 200 + 200);
        assert_eq!(summary.total_output_tokens, 50 + 25 + 100 + 100);
    }

    #[test]
    fn cost_summary_serde_roundtrip_includes_unknown_and_backfilled() {
        // Verify that unknown_pricing_count and backfilled_count survive
        // a serde round-trip (they must not be #[serde(default)]-dropped
        // or skipped).
        let summary = CostSummary {
            total_input_tokens: 500,
            total_output_tokens: 250,
            total_cache_read_tokens: 100,
            total_cache_creation_tokens: 50,
            total_reasoning_output_tokens: 20,
            total_cost_usd: 0.012,
            api_calls: 4,
            unknown_pricing_count: 2,
            backfilled_count: 3,
            by_model: vec![
                ModelCostSummary {
                    model: "gpt-4o".into(),
                    api_calls: 2,
                    total_tokens: 600,
                    total_cost_usd: 0.008,
                },
                ModelCostSummary {
                    model: "unknown".into(),
                    api_calls: 2,
                    total_tokens: 100,
                    total_cost_usd: 0.0,
                },
            ],
        };

        let json = serde_json::to_string(&summary).expect("serialize CostSummary");
        let deserialized: CostSummary =
            serde_json::from_str(&json).expect("deserialize CostSummary");

        assert_eq!(deserialized.unknown_pricing_count, 2);
        assert_eq!(deserialized.backfilled_count, 3);
        assert_eq!(deserialized.api_calls, 4);
        assert_eq!(deserialized.total_input_tokens, 500);
        assert!((deserialized.total_cost_usd - 0.012).abs() < 1e-12);
        assert_eq!(deserialized.by_model.len(), 2);
    }
}
