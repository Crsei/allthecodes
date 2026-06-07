//! Usage dashboard API handler.
//!
//! GET /api/usage?period=24h|7d|30d|90d|all&profile_id=
//!
//! Returns a usage dashboard response. Currently returns zeroed totals with
//! `partial: true` because no runtime usage-accumulation layer exists yet.
//!
//! Layered source strategy (not yet implemented):
//!   1. Runtime app state totals
//!   2. Session store aggregation
//!   3. Empty fallback (zero totals)

use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::handlers::ApiError;
use crate::state::WebState;

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

#[derive(Debug, Serialize)]
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
/// Returns `(StatusCode, Json<ApiError>)` on failure so callers can return it
/// directly as an `Err(...)` from the handler.
pub fn normalize_period(raw: &str) -> Result<String, (StatusCode, Json<ApiError>)> {
    let lower = raw.trim().to_lowercase();
    if VALID_PERIODS.contains(&lower.as_str()) {
        Ok(lower)
    } else {
        let valid = VALID_PERIODS.join(", ");
        Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: format!("invalid period '{}'. Must be one of: {}", raw, valid),
                code: "invalid_period".into(),

                details: serde_json::json!({}),
            }),
        ))
    }
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// GET /api/usage
pub async fn usage_handler(
    State(_state): State<WebState>,
    Query(query): Query<UsageQuery>,
) -> Result<Json<UsageDashboardResponse>, (StatusCode, Json<ApiError>)> {
    let period = match query.period.as_deref() {
        Some(p) if VALID_PERIODS.contains(&p) => p.to_string(),
        Some(invalid) => return Err(normalize_period(invalid).err().unwrap()),
        None => "7d".to_string(),
    };

    let generated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Phase 1: zeroed totals since usage accumulation is not yet implemented.
    Ok(Json(UsageDashboardResponse {
        profile_id: query.profile_id,
        period,
        generated_at,
        totals: UsageTotalsResponse {
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cache_read_tokens: 0,
            total_cache_creation_tokens: 0,
            total_cost_usd: 0.0,
            api_call_count: 0,
            session_count: None,
        },
        buckets: Vec::new(),
        by_model: Vec::new(),
        by_provider: Vec::new(),
        partial: true,
        warnings: vec!["Usage tracking is not yet implemented. All totals are zero.".to_string()],
    }))
}
