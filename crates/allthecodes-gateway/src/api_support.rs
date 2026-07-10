use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::{GatewayDiagnostic, GatewayError, GatewayLimits};

pub(crate) fn parse_json_limited<T: DeserializeOwned>(
    limits: &GatewayLimits,
    body: &[u8],
) -> Result<T, GatewayError> {
    if body.len() > limits.max_payload_bytes {
        return Err(GatewayError::new(GatewayDiagnostic::new(
            "payload_too_large",
            "The remote-control gateway payload is too large.",
            "Reduce the request body size and retry.",
        )));
    }
    serde_json::from_slice(body).map_err(|error| {
        GatewayError::new(
            GatewayDiagnostic::new(
                "invalid_json",
                "The remote-control gateway request body is not valid JSON.",
                "Send a JSON request body matching the endpoint schema.",
            )
            .with_context(format!("error={}", error)),
        )
    })
}

pub(crate) fn idempotency_header(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub fn error_response(error: GatewayError) -> Response {
    let diagnostic = error.into_diagnostic();
    let status = status_for_diagnostic(&diagnostic);
    (
        status,
        Json(json!({
            "error": diagnostic,
        })),
    )
        .into_response()
}

pub(crate) fn status_for_diagnostic(diagnostic: &GatewayDiagnostic) -> StatusCode {
    match diagnostic.code.as_str() {
        "missing_control_token"
        | "invalid_control_token"
        | "missing_remote_token"
        | "invalid_remote_token" => StatusCode::UNAUTHORIZED,
        "invalid_run_id" | "invalid_json" | "invalid_channel_config" | "channel_not_configured" => {
            StatusCode::BAD_REQUEST
        }
        "payload_too_large" => StatusCode::PAYLOAD_TOO_LARGE,
        "run_not_found" => StatusCode::NOT_FOUND,
        "replay_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
        "approval_id_missing"
        | "question_id_missing"
        | "adapter_unsupported"
        | "telegram_target_missing"
        | "lark_target_missing" => StatusCode::BAD_REQUEST,
        "busy" | "stale_response" | "run_already_terminal" => StatusCode::CONFLICT,
        "bad_hmac"
        | "bad_origin"
        | "missing_signature"
        | "remote_token_required"
        | "telegram_target_blocked"
        | "webhook_disabled"
        | "lark_target_blocked" => StatusCode::FORBIDDEN,
        "queue_full" | "rate_limited" => StatusCode::TOO_MANY_REQUESTS,
        "unsupported" | "telegram_transport_unavailable" | "lark_transport_unavailable" => {
            StatusCode::NOT_IMPLEMENTED
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
