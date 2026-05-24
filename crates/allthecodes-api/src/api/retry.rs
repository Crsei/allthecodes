//! Retry logic with exponential backoff and model fallback
use std::time::Duration;

/// Retry configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: usize,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub backoff_multiplier: f64,
    pub retryable_status_codes: Vec<u16>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
            backoff_multiplier: 2.0,
            retryable_status_codes: vec![429, 500, 502, 503, 529],
        }
    }
}

/// Categorized API error for retry decisions
#[derive(Debug, Clone)]
pub enum ApiErrorCategory {
    /// Rate limited 鈥?retry with backoff
    RateLimit { retry_after_ms: Option<u64> },
    /// Server overloaded 鈥?retry with backoff, maybe fallback
    Overloaded,
    /// Server error 鈥?retry
    ServerError,
    /// Invalid request 鈥?don't retry
    InvalidRequest { message: String },
    /// Auth error 鈥?don't retry
    AuthError,
    /// Prompt too long 鈥?don't retry (handle differently)
    PromptTooLong,
    /// Max output tokens 鈥?don't retry (handle differently)
    MaxOutputTokens,
    /// Unknown 鈥?don't retry
    Unknown {
        status: Option<u16>,
        message: String,
    },
}

impl ApiErrorCategory {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimit { .. } | Self::Overloaded | Self::ServerError
        )
    }
}

/// Categorize an API error response
pub fn categorize_api_error(status: u16, body: &str) -> ApiErrorCategory {
    match status {
        429 => {
            // Check for overloaded vs rate limit
            if body.contains("overloaded") {
                ApiErrorCategory::Overloaded
            } else {
                ApiErrorCategory::RateLimit {
                    retry_after_ms: None,
                }
            }
        }
        529 => ApiErrorCategory::Overloaded,
        500..=599 => ApiErrorCategory::ServerError,
        400 => {
            if body.contains("prompt is too long") || body.contains("too many tokens") {
                ApiErrorCategory::PromptTooLong
            } else if body.contains("max_tokens") {
                ApiErrorCategory::MaxOutputTokens
            } else {
                ApiErrorCategory::InvalidRequest {
                    message: body.to_string(),
                }
            }
        }
        401 | 403 => ApiErrorCategory::AuthError,
        _ => ApiErrorCategory::Unknown {
            status: Some(status),
            message: body.to_string(),
        },
    }
}

/// Categorize a failure that happened before the stream was handed to callers.
///
/// Provider implementations currently return `anyhow::Error`, so this parser
/// preserves retry semantics across Anthropic, OpenAI-compatible, Gemini,
/// Vertex, and Bedrock error strings until those paths grow typed errors.
pub fn categorize_stream_start_error(message: &str) -> ApiErrorCategory {
    if let Some(status) = extract_http_status(message) {
        return categorize_api_error(status, message);
    }

    let lower = message.to_ascii_lowercase();
    if lower.contains("prompt is too long")
        || lower.contains("prompt_too_long")
        || lower.contains("too many tokens")
    {
        return ApiErrorCategory::PromptTooLong;
    }
    if lower.contains("max_tokens") || lower.contains("max output tokens") {
        return ApiErrorCategory::MaxOutputTokens;
    }
    if lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("invalid api key")
        || lower.contains("authentication")
    {
        return ApiErrorCategory::AuthError;
    }
    if lower.contains("invalid request") || lower.contains("bad request") {
        return ApiErrorCategory::InvalidRequest {
            message: message.to_string(),
        };
    }
    if lower.contains("overloaded") || lower.contains("high demand") || lower.contains("capacity") {
        return ApiErrorCategory::Overloaded;
    }
    if lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("too many requests")
    {
        return ApiErrorCategory::RateLimit {
            retry_after_ms: None,
        };
    }
    if lower.contains("failed to send")
        || lower.contains("error sending")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("connection")
        || lower.contains("connect")
        || lower.contains("dns")
        || lower.contains("eof")
        || lower.contains("network")
    {
        return ApiErrorCategory::ServerError;
    }

    ApiErrorCategory::Unknown {
        status: None,
        message: message.to_string(),
    }
}

fn extract_http_status(message: &str) -> Option<u16> {
    let lower = message.to_ascii_lowercase();
    for marker in ["http ", "status ", "status: ", "status="] {
        if let Some(index) = lower.find(marker) {
            let after = &lower[index + marker.len()..];
            let digits: String = after
                .chars()
                .skip_while(|ch| !ch.is_ascii_digit())
                .take_while(|ch| ch.is_ascii_digit())
                .collect();
            if digits.len() == 3 {
                if let Ok(status) = digits.parse::<u16>() {
                    return Some(status);
                }
            }
        }
    }
    None
}

/// Calculate delay for a retry attempt
pub fn retry_delay(config: &RetryConfig, attempt: usize) -> Duration {
    let delay = config.initial_delay_ms as f64 * config.backoff_multiplier.powi(attempt as i32);
    let delay = delay.min(config.max_delay_ms as f64) as u64;
    // Add jitter (卤20%)
    let jitter = (delay as f64 * 0.2 * (rand_fraction() * 2.0 - 1.0)) as i64;
    Duration::from_millis((delay as i64 + jitter).max(0) as u64)
}

fn rand_fraction() -> f64 {
    // Simple pseudo-random for jitter 鈥?not crypto-secure
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos % 1000) as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_start_error_classifies_retryable_http_and_network_failures() {
        assert!(
            categorize_stream_start_error("Provider qwen error (HTTP 429): rate limit")
                .is_retryable()
        );
        assert!(
            categorize_stream_start_error("Google Gemini error (HTTP 504): gateway timeout")
                .is_retryable()
        );
        assert!(
            categorize_stream_start_error(
                "API error provider=anthropic status=529 request_id=req type=overloaded_error: overloaded"
            )
            .is_retryable()
        );
        assert!(
            categorize_stream_start_error("failed to send HTTP request: connection closed")
                .is_retryable()
        );
        assert!(categorize_stream_start_error("529 overloaded: high demand").is_retryable());
    }

    #[test]
    fn stream_start_error_keeps_nonretryable_failures_terminal() {
        assert!(
            !categorize_stream_start_error("API error (HTTP 400): prompt is too long")
                .is_retryable()
        );
        assert!(
            !categorize_stream_start_error("API error (HTTP 401): unauthorized").is_retryable()
        );
        assert!(!categorize_stream_start_error("Provider error: invalid request").is_retryable());
        assert!(
            !categorize_stream_start_error("provider returned malformed content").is_retryable()
        );
    }
}
