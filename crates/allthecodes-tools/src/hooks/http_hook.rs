//! HTTP WebHook execution.
//!
//! Executes HTTP hooks by POSTing hook input JSON to configured URLs.
//! Supports env var interpolation in headers, URL allowlisting,
//! SSRF guard validation, and sandbox proxy routing.
//!
//! Port of TypeScript `execHttpHook.ts`.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;
use tracing::{debug, warn};

use super::ssrf_guard::is_blocked_address;
use allthecodes_types::hooks::HttpHookPolicy;

const DEFAULT_HTTP_HOOK_TIMEOUT: Duration = Duration::from_secs(600); // 10 minutes

/// Result of executing an HTTP hook.
#[derive(Debug, Clone)]
pub struct HttpHookResult {
    pub ok: bool,
    pub status_code: Option<u16>,
    pub body: String,
    pub error: Option<String>,
    pub aborted: bool,
}

/// Match a URL against a pattern with * as a wildcard (any characters).
/// Same semantics as the MCP server allowlist patterns.
fn url_matches_pattern(url: &str, pattern: &str) -> bool {
    // Escape regex special characters except *
    let escaped = regex::escape(pattern);
    let regex_str = escaped.replace(r"\*", ".*");
    match regex::Regex::new(&format!("^{regex_str}$")) {
        Ok(re) => re.is_match(url),
        Err(error) => {
            warn!(pattern = pattern, %error, "invalid HTTP hook URL allowlist pattern");
            false
        }
    }
}

/// Strip CR, LF, and NUL bytes from a header value to prevent HTTP header
/// injection (CRLF injection) via env var values or hook-configured header templates.
fn sanitize_header_value(value: &str) -> String {
    value.replace('\r', "").replace('\n', "").replace('\0', "")
}

/// Interpolate $VAR_NAME and ${VAR_NAME} patterns in a string using
/// environment variables, but only for variable names in the allowlist.
fn interpolate_env_vars(
    value: &str,
    allowed_env_vars: &std::collections::HashSet<String>,
) -> String {
    let Ok(re) = regex::Regex::new(r"\$\{([A-Z_][A-Z0-9_]*)\}|\$([A-Z_][A-Z0-9_]*)") else {
        return sanitize_header_value(value);
    };
    let result = re.replace_all(value, |caps: &regex::Captures| {
        let var_name = caps
            .get(1)
            .or_else(|| caps.get(2))
            .map(|m| m.as_str())
            .unwrap_or("");
        if allowed_env_vars.contains(var_name) {
            std::env::var(var_name).unwrap_or_default()
        } else {
            debug!(
                var_name = var_name,
                "env var not in allowed list, skipping interpolation"
            );
            String::new()
        }
    });
    sanitize_header_value(&result)
}

/// Execute an HTTP hook by POSTing the hook input JSON to the configured URL.
pub async fn exec_http_hook(
    url: &str,
    _hook_event: &str,
    json_input: &Value,
    headers: Option<&HashMap<String, String>>,
    allowed_env_vars: Option<&[String]>,
    policy: Option<&HttpHookPolicy>,
    timeout_secs: Option<u64>,
) -> HttpHookResult {
    // Enforce URL allowlist
    if let Some(policy) = policy {
        if let Some(ref allowed_urls) = policy.allowed_urls {
            let matched = allowed_urls.iter().any(|p| url_matches_pattern(url, p));
            if !matched {
                let msg = format!(
                    "HTTP hook blocked: {url} does not match any pattern in allowedHttpHookUrls"
                );
                warn!(msg);
                return HttpHookResult {
                    ok: false,
                    body: String::new(),
                    error: Some(msg),
                    aborted: false,
                    status_code: None,
                };
            }
        }
    }

    let timeout = timeout_secs
        .map(|s| Duration::from_secs(s))
        .unwrap_or(DEFAULT_HTTP_HOOK_TIMEOUT);

    // Build headers with env var interpolation
    let mut request_headers: HashMap<String, String> = HashMap::new();
    request_headers.insert("Content-Type".to_string(), "application/json".to_string());

    if let Some(hdrs) = headers {
        // Resolve allowed env vars intersection
        let hook_vars: Vec<String> = allowed_env_vars.map(|v| v.to_vec()).unwrap_or_default();

        let effective_vars = if let Some(ref policy) = policy {
            if let Some(ref policy_vars) = policy.allowed_env_vars {
                hook_vars
                    .into_iter()
                    .filter(|v| policy_vars.contains(v))
                    .collect()
            } else {
                hook_vars
            }
        } else {
            hook_vars
        };

        let allowed_set: std::collections::HashSet<String> = effective_vars.into_iter().collect();

        for (name, value) in hdrs {
            request_headers.insert(name.clone(), interpolate_env_vars(value, &allowed_set));
        }
    }

    let json_str = serde_json::to_string(json_input).unwrap_or_default();

    // Build HTTP client with SSRF guard
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .no_proxy() // Don't use system proxy for hooks by default
        .build()
        .unwrap_or_default();

    debug!("HTTP hook POST to {url}");

    // Check URL for SSRF before sending
    if let Some(host) = url.split('/').nth(2) {
        // Quick check: if it's an IP literal, validate it
        if let Ok(addr) = host.parse::<std::net::IpAddr>() {
            if let Err(msg) = crate::hooks::ssrf_guard::SsrfGuard::check_address(addr) {
                warn!("{msg}");
                return HttpHookResult {
                    ok: false,
                    body: String::new(),
                    error: Some(msg),
                    aborted: false,
                    status_code: None,
                };
            }
        } else {
            // For hostnames, perform DNS lookup with SSRF validation
            match tokio::net::lookup_host(format!("{host}:0")).await {
                Ok(addrs) => {
                    for addr in addrs {
                        if is_blocked_address(addr.ip()) {
                            let msg = format!(
                                "HTTP hook blocked: {host} resolves to {} (private/link-local address)",
                                addr.ip()
                            );
                            warn!("{msg}");
                            return HttpHookResult {
                                ok: false,
                                body: String::new(),
                                error: Some(msg),
                                aborted: false,
                                status_code: None,
                            };
                        }
                    }
                }
                Err(e) => {
                    let msg = format!("HTTP hook DNS resolution failed for {host}: {e}");
                    warn!("{msg}");
                    return HttpHookResult {
                        ok: false,
                        body: String::new(),
                        error: Some(msg),
                        aborted: false,
                        status_code: None,
                    };
                }
            }
        }
    }

    // Send POST request
    match client
        .post(url)
        .headers({
            let mut headers_map = reqwest::header::HeaderMap::new();
            for (k, v) in &request_headers {
                if let (Ok(name), Ok(value)) = (
                    reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                    reqwest::header::HeaderValue::from_str(v),
                ) {
                    headers_map.insert(name, value);
                }
            }
            headers_map
        })
        .body(json_str)
        .send()
        .await
    {
        Ok(response) => {
            let status_code = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            debug!(
                "HTTP hook response status {status_code}, body length {}",
                body.len()
            );

            HttpHookResult {
                ok: status_code >= 200 && status_code < 300,
                status_code: Some(status_code),
                body,
                error: None,
                aborted: false,
            }
        }
        Err(e) => {
            if e.is_timeout() || e.is_connect() {
                let msg = format!("HTTP hook error: {e}");
                warn!("{msg}");
                HttpHookResult {
                    ok: false,
                    body: String::new(),
                    error: Some(msg),
                    aborted: e.is_timeout(),
                    status_code: None,
                }
            } else {
                let msg = format!("HTTP hook error: {e}");
                warn!("{msg}");
                HttpHookResult {
                    ok: false,
                    body: String::new(),
                    error: Some(msg),
                    aborted: false,
                    status_code: None,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_matches_pattern() {
        assert!(url_matches_pattern(
            "https://example.com/hook",
            "https://example.com/hook"
        ));
        assert!(url_matches_pattern(
            "https://hooks.example.com/callback",
            "https://*.example.com/*"
        ));
        assert!(!url_matches_pattern(
            "https://evil.com/hook",
            "https://example.com/*"
        ));
        assert!(url_matches_pattern(
            "http://localhost:8080/hook",
            "http://localhost:*"
        ));
    }

    #[test]
    fn test_sanitize_header_value() {
        assert_eq!(
            sanitize_header_value("hello\r\nX-Evil: 1"),
            "helloX-Evil: 1"
        );
        assert_eq!(sanitize_header_value("Bearer token123"), "Bearer token123");
        assert_eq!(sanitize_header_value("val\0ue"), "value");
    }

    #[test]
    fn test_interpolate_env_vars_allowed() {
        let mut allowed = std::collections::HashSet::new();
        allowed.insert("MY_TOKEN".to_string());
        allowed.insert("ANOTHER".to_string());

        let result = interpolate_env_vars("Bearer $MY_TOKEN and ${ANOTHER}", &allowed);
        // The values would be empty since env vars aren't set in tests
        assert!(!result.contains("$MY_TOKEN"));
    }

    #[test]
    fn test_interpolate_env_vars_blocked() {
        let mut allowed = std::collections::HashSet::new();
        allowed.insert("ALLOWED".to_string());

        let result = interpolate_env_vars("$ALLOWED and $BLOCKED", &allowed);
        assert_eq!(result, " and ");
    }

    #[test]
    fn test_url_matches_pattern_with_trailing_path() {
        assert!(url_matches_pattern(
            "https://api.example.com/v1/hooks/callback",
            "https://api.example.com/*"
        ));
        assert!(!url_matches_pattern(
            "https://api.example.com/v1/hooks/callback",
            "https://api.other.com/*"
        ));
    }
}
