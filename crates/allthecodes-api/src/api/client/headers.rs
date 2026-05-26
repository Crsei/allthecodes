//! Anthropic HTTP header construction.

use std::collections::HashMap;

use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use serde_json::Value;

use super::types::AnthropicAuth;

// ---------------------------------------------------------------------------
// Public header builders
// ---------------------------------------------------------------------------

/// Build standard Anthropic-format headers for requests that do not
/// inspect the JSON body.
pub(crate) fn build_anthropic_headers(
    auth: &AnthropicAuth,
    include_token_counting_beta: bool,
) -> Result<HeaderMap> {
    build_anthropic_headers_with_cache_betas(
        auth,
        include_token_counting_beta,
        true,
        false,
        false,
        false,
        true,
    )
}

/// Build headers, inspecting the JSON body to automatically decide which
/// beta feature headers are needed.
pub(crate) fn build_anthropic_headers_for_body(
    auth: &AnthropicAuth,
    include_token_counting_beta: bool,
    body: &Value,
) -> Result<HeaderMap> {
    build_anthropic_headers_for_body_with_beta_policy(auth, include_token_counting_beta, body, true)
}

/// Like [`build_anthropic_headers_for_body`] but allows callers to control
/// whether the `anthropic-beta` header is included at all.
pub(crate) fn build_anthropic_headers_for_body_with_beta_policy(
    auth: &AnthropicAuth,
    include_token_counting_beta: bool,
    body: &Value,
    include_anthropic_beta_header: bool,
) -> Result<HeaderMap> {
    build_anthropic_headers_with_cache_betas(
        auth,
        include_token_counting_beta,
        body_contains_key(body, "cache_control"),
        body_contains_cache_attr(body, "ttl"),
        body_contains_cache_attr(body, "scope"),
        body_contains_output_effort(body),
        include_anthropic_beta_header,
    )
}

// ---------------------------------------------------------------------------
// Internal header helpers
// ---------------------------------------------------------------------------

fn build_anthropic_headers_with_cache_betas(
    auth: &AnthropicAuth,
    include_token_counting_beta: bool,
    include_prompt_cache_beta: bool,
    include_ttl_beta: bool,
    include_global_scope_beta: bool,
    include_effort_beta: bool,
    include_anthropic_beta_header: bool,
) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(&allthecodes_config::user_agent::api_user_agent())
            .context("failed to build User-Agent header")?,
    );
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    if include_anthropic_beta_header {
        let mut betas = vec!["interleaved-thinking-2025-05-14"];
        if include_prompt_cache_beta {
            betas.push("prompt-caching-2024-07-16");
        }
        if include_ttl_beta {
            betas.push("extended-cache-ttl-2025-04-11");
        }
        if include_global_scope_beta {
            betas.push("prompt-caching-scope-2026-01-05");
        }
        if include_effort_beta {
            betas.push(allthecodes_config::constants::api::EFFORT_BETA);
        }
        if include_token_counting_beta {
            betas.push("token-counting-2024-11-01");
        }
        let beta = HeaderValue::from_str(&betas.join(","))
            .context("failed to build anthropic-beta header")?;
        headers.insert("anthropic-beta", beta);
    }
    auth.insert_auth_header(&mut headers)?;
    Ok(headers)
}

fn body_contains_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(map) => {
            map.contains_key(key) || map.values().any(|v| body_contains_key(v, key))
        }
        Value::Array(values) => values.iter().any(|v| body_contains_key(v, key)),
        _ => false,
    }
}

fn body_contains_output_effort(value: &Value) -> bool {
    value
        .get("output_config")
        .and_then(|config| config.get("effort"))
        .is_some()
}

fn body_contains_cache_attr(value: &Value, attr: &str) -> bool {
    match value {
        Value::Object(map) => {
            map.get("cache_control")
                .and_then(Value::as_object)
                .and_then(|cache| cache.get(attr))
                .is_some()
                || map.values().any(|v| body_contains_cache_attr(v, attr))
        }
        Value::Array(values) => values.iter().any(|v| body_contains_cache_attr(v, attr)),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Header map extension helper (used by messages.rs)
// ---------------------------------------------------------------------------

/// Copy entries from a [`HeaderMap`] into a plain [`HashMap`].
pub(super) fn extend_header_string_map(
    map: &mut HashMap<String, String>,
    headers: &HeaderMap,
) {
    for (name, value) in headers {
        if let Ok(value) = value.to_str() {
            let name = if name == reqwest::header::AUTHORIZATION {
                "Authorization"
            } else {
                name.as_str()
            };
            map.insert(name.to_string(), value.to_string());
        }
    }
}
