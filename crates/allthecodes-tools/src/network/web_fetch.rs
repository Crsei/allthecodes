//! WebFetch tool — fetch content from a URL and convert HTML to plain text.
//!
//! Corresponds to TypeScript: tools/WebFetchTool/WebFetchTool.ts
//!
//! Features:
//! - HTTP GET with configurable timeout
//! - HTML → plain-text extraction (strip tags)
//! - Content truncation (max 100 KB)
//! - LRU response cache (15 min TTL)
//! - URL validation and HTTPS upgrade

use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::*;
use allthecodes_sandbox::{policy_from_app_state, NetworkDecision};
use allthecodes_types::message::AssistantMessage;

// ---------------------------------------------------------------------------
// Constants (matching TypeScript source)
// ---------------------------------------------------------------------------

/// Maximum length of extracted text returned to the model.
const MAX_TEXT_LENGTH: usize = 100_000;

/// HTTP request timeout.
const FETCH_TIMEOUT: Duration = Duration::from_secs(60);

/// Maximum response body size (10 MB).
const MAX_CONTENT_LENGTH: usize = 10 * 1024 * 1024;

/// Maximum URL length.
const MAX_URL_LENGTH: usize = 2_000;

/// Cache TTL (15 minutes).
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);

/// Maximum number of cached entries.
const MAX_CACHE_ENTRIES: usize = 64;

/// Maximum same-host redirect hops.
const MAX_REDIRECTS: usize = 10;

// ---------------------------------------------------------------------------
// Simple in-memory cache
// ---------------------------------------------------------------------------

struct CacheEntry {
    content: String,
    status: u16,
    fetched_at: Instant,
}

static CACHE: std::sync::LazyLock<Mutex<HashMap<String, CacheEntry>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn cache_get(url: &str) -> Option<(String, u16)> {
    let cache = CACHE.lock();
    let entry = cache.get(url)?;
    if entry.fetched_at.elapsed() < CACHE_TTL {
        Some((entry.content.clone(), entry.status))
    } else {
        None
    }
}

fn cache_put(url: &str, content: &str, status: u16) {
    {
        let mut cache = CACHE.lock();
        // Evict oldest if over capacity
        if cache.len() >= MAX_CACHE_ENTRIES {
            let oldest_key = cache
                .iter()
                .min_by_key(|(_, v)| v.fetched_at)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest_key {
                cache.remove(&k);
            }
        }
        cache.insert(
            url.to_string(),
            CacheEntry {
                content: content.to_string(),
                status,
                fetched_at: Instant::now(),
            },
        );
    }
}

// ---------------------------------------------------------------------------
// HTML → text helpers
// ---------------------------------------------------------------------------

/// Naive HTML tag stripping — removes all `<…>` tags and decodes common
/// entities.  Good enough for extracting readable text from web pages without
/// pulling in a full HTML parser crate.
fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let bytes = html.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let ch = bytes[i];
        if ch == b'<' {
            // Detect <script and <style blocks
            let rest = &html[i..];
            if rest.len() > 7 && rest[..7].eq_ignore_ascii_case("<script") {
                in_script = true;
            } else if rest.len() > 6 && rest[..6].eq_ignore_ascii_case("<style") {
                in_style = true;
            } else if rest.len() > 8 && rest[..9].eq_ignore_ascii_case("</script>") {
                in_script = false;
            } else if rest.len() > 7 && rest[..8].eq_ignore_ascii_case("</style>") {
                in_style = false;
            }
            in_tag = true;
            i += 1;
            continue;
        }
        if ch == b'>' {
            in_tag = false;
            i += 1;
            continue;
        }
        if !in_tag && !in_script && !in_style {
            out.push(ch as char);
        }
        i += 1;
    }

    // Decode common HTML entities
    let out = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");

    // Collapse whitespace runs
    collapse_whitespace(&out)
}

/// Collapse runs of whitespace (spaces, tabs, newlines) into single spaces,
/// then collapse 3+ newlines into 2.
fn collapse_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_ws = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if ch == '\n' {
                if !prev_ws {
                    result.push('\n');
                }
                prev_ws = true;
            } else if !prev_ws {
                result.push(' ');
                prev_ws = true;
            }
        } else {
            prev_ws = false;
            result.push(ch);
        }
    }
    result.trim().to_string()
}

/// Truncate text to `max_len` characters with a notice if truncated.
fn truncate_text(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        return text.to_string();
    }
    let half = max_len / 2;
    let head = &text[..half];
    let tail = &text[text.len() - half..];
    format!(
        "{}\n\n[... {} characters omitted ...]\n\n{}",
        head,
        text.len() - max_len,
        tail
    )
}

// ---------------------------------------------------------------------------
// URL helpers
// ---------------------------------------------------------------------------

/// Validate and normalise a URL string.
fn normalise_url(raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        bail!("URL is empty");
    }
    if raw.len() > MAX_URL_LENGTH {
        bail!("URL exceeds {} character limit", MAX_URL_LENGTH);
    }

    // Upgrade http → https
    let url = if raw.starts_with("http://") {
        raw.replacen("http://", "https://", 1)
    } else if !raw.starts_with("https://") {
        format!("https://{}", raw)
    } else {
        raw.to_string()
    };

    // Basic parse check
    let parsed = url::Url::parse(&url).context("Invalid URL")?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        bail!(
            "URL contains embedded credentials; WebFetch does not support cookie or credentialed URL fetches"
        );
    }
    Ok(url)
}

fn is_redirect_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 301 | 302 | 307 | 308)
}

fn redirect_status_text(status: u16) -> &'static str {
    match status {
        301 => "Moved Permanently",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        302 => "Found",
        _ => "Redirect",
    }
}

fn is_permitted_redirect(original_url: &str, redirect_url: &str) -> bool {
    let Ok(parsed_original) = url::Url::parse(original_url) else {
        return false;
    };
    let Ok(parsed_redirect) = url::Url::parse(redirect_url) else {
        return false;
    };

    if parsed_redirect.scheme() != parsed_original.scheme() {
        return false;
    }
    if parsed_redirect.port() != parsed_original.port() {
        return false;
    }
    if !parsed_redirect.username().is_empty() || parsed_redirect.password().is_some() {
        return false;
    }

    let strip_www = |host: &str| host.strip_prefix("www.").unwrap_or(host).to_string();
    let Some(original_host) = parsed_original.host_str() else {
        return false;
    };
    let Some(redirect_host) = parsed_redirect.host_str() else {
        return false;
    };
    strip_www(original_host) == strip_www(redirect_host)
}

fn resolve_redirect_url(original_url: &str, location: &str) -> Result<String> {
    let base = url::Url::parse(original_url).context("Invalid redirect base URL")?;
    Ok(base
        .join(location)
        .context("Invalid redirect Location")?
        .to_string())
}

enum FetchOutcome {
    Response(reqwest::Response),
    Redirect {
        original_url: String,
        redirect_url: String,
        status: u16,
    },
}

async fn get_with_permitted_redirects(
    client: &reqwest::Client,
    initial_url: &str,
) -> Result<FetchOutcome> {
    let mut current_url = initial_url.to_string();
    let mut redirect_count = 0usize;

    loop {
        let resp = client
            .get(&current_url)
            .header("Accept", "text/html,application/xhtml+xml,text/plain,*/*")
            .send()
            .await
            .with_context(|| format!("Failed to fetch {}", current_url))?;

        if !is_redirect_status(resp.status()) {
            return Ok(FetchOutcome::Response(resp));
        }

        let status = resp.status().as_u16();
        let Some(location) = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
        else {
            bail!(
                "Redirect response from {} missing Location header",
                current_url
            );
        };
        let redirect_url = resolve_redirect_url(&current_url, location)?;

        if !is_permitted_redirect(&current_url, &redirect_url) {
            return Ok(FetchOutcome::Redirect {
                original_url: current_url,
                redirect_url,
                status,
            });
        }

        redirect_count += 1;
        if redirect_count > MAX_REDIRECTS {
            bail!("Too many redirects (exceeded {})", MAX_REDIRECTS);
        }
        current_url = redirect_url;
    }
}

fn first_env_value<F>(names: &[&str], lookup: &F) -> Option<String>
where
    F: Fn(&str) -> Option<String>,
{
    names
        .iter()
        .find_map(|name| lookup(name).filter(|value| !value.trim().is_empty()))
}

fn no_proxy_matches(host: &str, no_proxy: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    no_proxy.split(',').any(|pattern| {
        let pattern = pattern.trim().trim_end_matches('.').to_ascii_lowercase();
        if pattern.is_empty() {
            return false;
        }
        if pattern == "*" {
            return true;
        }
        let pattern = pattern
            .split_once(':')
            .map(|(host, _)| host)
            .unwrap_or(pattern.as_str())
            .trim_start_matches('.');
        host == pattern || host.ends_with(&format!(".{}", pattern))
    })
}

fn proxy_url_from_env_for_url(url: &str) -> Option<String> {
    proxy_url_from_env_for_url_with(url, |name| std::env::var(name).ok())
}

fn proxy_url_from_env_for_url_with<F>(url: &str, lookup: F) -> Option<String>
where
    F: Fn(&str) -> Option<String>,
{
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    if first_env_value(&["NO_PROXY", "no_proxy"], &lookup)
        .as_deref()
        .is_some_and(|no_proxy| no_proxy_matches(host, no_proxy))
    {
        return None;
    }

    let candidates: &[&str] = if parsed.scheme() == "https" {
        &[
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
            "HTTP_PROXY",
            "http_proxy",
        ]
    } else {
        &[
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
            "HTTPS_PROXY",
            "https_proxy",
        ]
    };
    first_env_value(candidates, &lookup)
}

fn apply_proxy_from_env(
    builder: reqwest::ClientBuilder,
    url: &str,
) -> Result<reqwest::ClientBuilder> {
    let Some(proxy_url) = proxy_url_from_env_for_url(url) else {
        return Ok(builder);
    };
    let proxy = reqwest::Proxy::all(&proxy_url)
        .with_context(|| format!("Invalid proxy URL {proxy_url}"))?;
    Ok(builder.proxy(proxy))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseContentKind {
    Html,
    Json,
    Text,
    Binary,
}

fn media_type(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

fn is_binary_content_type(content_type: &str) -> bool {
    let media_type = media_type(content_type);
    if media_type.is_empty() || media_type.starts_with("text/") {
        return false;
    }
    if media_type == "application/json" || media_type.ends_with("+json") {
        return false;
    }
    if media_type == "application/xml" || media_type.ends_with("+xml") {
        return false;
    }
    if media_type.starts_with("application/javascript") {
        return false;
    }
    if media_type == "application/x-www-form-urlencoded" {
        return false;
    }
    true
}

fn response_content_kind(content_type: &str) -> ResponseContentKind {
    let media_type = media_type(content_type);
    if media_type == "text/html" || media_type == "application/xhtml+xml" {
        ResponseContentKind::Html
    } else if media_type == "application/json" || media_type.ends_with("+json") {
        ResponseContentKind::Json
    } else if is_binary_content_type(content_type) {
        ResponseContentKind::Binary
    } else {
        ResponseContentKind::Text
    }
}

fn extract_text_for_content_type(content_type: &str, body_bytes: &[u8]) -> String {
    match response_content_kind(content_type) {
        ResponseContentKind::Html => {
            let body = String::from_utf8_lossy(body_bytes);
            strip_html_tags(&body)
        }
        ResponseContentKind::Json => serde_json::from_slice::<Value>(body_bytes)
            .ok()
            .and_then(|value| serde_json::to_string_pretty(&value).ok())
            .unwrap_or_else(|| String::from_utf8_lossy(body_bytes).to_string()),
        ResponseContentKind::Text | ResponseContentKind::Binary => {
            String::from_utf8_lossy(body_bytes).to_string()
        }
    }
}

// ---------------------------------------------------------------------------
// Tool implementation
// ---------------------------------------------------------------------------

pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "WebFetch"
    }

    async fn description(&self, _: &Value) -> String {
        "Fetch a web page and extract its text content.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch. Must be a valid HTTP/HTTPS URL."
                },
                "prompt": {
                    "type": "string",
                    "description": "Optional instructions for content extraction (e.g. 'extract the main article text')."
                }
            },
            "required": ["url"]
        })
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let url = input.get("url").and_then(|v| v.as_str()).unwrap_or("");
        if url.is_empty() {
            return ValidationResult::Error {
                message: "url is required".to_string(),
                error_code: 400,
            };
        }
        if url.len() > MAX_URL_LENGTH {
            return ValidationResult::Error {
                message: format!("URL exceeds {} character limit", MAX_URL_LENGTH),
                error_code: 400,
            };
        }
        if let Err(err) = normalise_url(url) {
            return ValidationResult::Error {
                message: format!("Invalid URL: {err}"),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let raw_url = input.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let _prompt = input.get("prompt").and_then(|v| v.as_str()).unwrap_or("");

        let url = normalise_url(raw_url)?;

        // Sandbox network policy check (runs before the cache so that a
        // configuration change is noticed immediately rather than returning
        // stale-but-allowed content).
        let app_state = (ctx.get_app_state)();
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let policy = policy_from_app_state(
            &app_state.tool_permission_context,
            &app_state.settings.sandbox,
            cwd,
            false,
        );
        if let NetworkDecision::Denied(err) = policy.network.check_url(&url) {
            return Ok(ToolResult {
                data: json!({
                    "url": url,
                    "error": err.to_string(),
                    "sandbox_blocked": true,
                }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        // Check cache first
        if let Some((cached, status)) = cache_get(&url) {
            return Ok(ToolResult {
                data: json!({
                    "url": url,
                    "status": status,
                    "content": cached,
                    "cached": true,
                }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        let start = Instant::now();

        let client = reqwest::Client::builder()
            .timeout(FETCH_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(allthecodes_config::user_agent::web_fetch_user_agent());
        let client = apply_proxy_from_env(client, &url)?
            .build()
            .context("Failed to build HTTP client")?;

        let resp = match get_with_permitted_redirects(&client, &url).await? {
            FetchOutcome::Response(resp) => resp,
            FetchOutcome::Redirect {
                original_url,
                redirect_url,
                status,
            } => {
                let status_text = redirect_status_text(status);
                let message = format!(
                    "REDIRECT DETECTED: The URL redirects to a different host.\n\n\
Original URL: {original_url}\n\
Redirect URL: {redirect_url}\n\
Status: {status} {status_text}\n\n\
To complete your request, use WebFetch again with the redirected URL."
                );
                return Ok(ToolResult {
                    data: json!({
                        "url": url,
                        "status": status,
                        "content": message,
                        "redirect_detected": true,
                        "original_url": original_url,
                        "redirect_url": redirect_url,
                        "durationMs": start.elapsed().as_millis() as u64,
                    }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
        };

        let status = resp.status().as_u16();
        let final_url = resp.url().to_string();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        // Read body with size limit
        let body_bytes = resp.bytes().await.context("Failed to read response body")?;

        if body_bytes.len() > MAX_CONTENT_LENGTH {
            return Ok(ToolResult {
                data: json!({
                    "url": url,
                    "status": status,
                    "error": format!(
                        "Response body too large ({} bytes, limit {})",
                        body_bytes.len(),
                        MAX_CONTENT_LENGTH
                    ),
                }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        let content_kind = response_content_kind(&content_type);
        if content_kind == ResponseContentKind::Binary {
            return Ok(ToolResult {
                data: json!({
                    "url": url,
                    "finalUrl": final_url,
                    "status": status,
                    "contentType": content_type,
                    "bytes": body_bytes.len(),
                    "binary": true,
                    "error": "WebFetch received binary content and did not place raw bytes into the model context",
                }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        let text = extract_text_for_content_type(&content_type, &body_bytes);
        let text = truncate_text(&text, MAX_TEXT_LENGTH);
        let duration_ms = start.elapsed().as_millis() as u64;

        // Cache successful responses
        if (200..400).contains(&status) {
            cache_put(&url, &text, status);
        }

        Ok(ToolResult {
            data: json!({
                "url": url,
                "finalUrl": final_url,
                "status": status,
                "content": text,
                "contentType": content_type,
                "bytes": body_bytes.len(),
                "durationMs": duration_ms,
            }),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        r#"Fetch content from a URL and extract its text. The tool:
- Converts HTML to readable text (strips tags, scripts, styles)
- Truncates results to 100,000 characters
- Caches responses for 15 minutes
- Upgrades HTTP to HTTPS automatically
- Supports a `prompt` parameter for extraction instructions

Use this tool when you need to read web page content. For APIs returning JSON,
the raw response is returned as-is."#
            .to_string()
    }

    fn user_facing_name(&self, input: Option<&Value>) -> String {
        if let Some(url) = input.and_then(|v| v.get("url")).and_then(|v| v.as_str()) {
            // Show just the domain
            if let Ok(parsed) = url::Url::parse(url) {
                if let Some(host) = parsed.host_str() {
                    return format!("WebFetch({})", host);
                }
            }
            return format!("WebFetch({})", &url[..url.len().min(40)]);
        }
        "WebFetch".to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html_basic() {
        let html = "<html><body><h1>Hello</h1><p>World</p></body></html>";
        let text = strip_html_tags(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
        assert!(!text.contains("<"));
    }

    #[test]
    fn test_strip_html_script_style() {
        let html = "<div>Before<script>var x = 1;</script>After<style>.a{}</style>End</div>";
        let text = strip_html_tags(html);
        assert!(text.contains("Before"));
        assert!(text.contains("After"));
        assert!(text.contains("End"));
        assert!(!text.contains("var x"));
        assert!(!text.contains(".a{"));
    }

    #[test]
    fn test_strip_html_entities() {
        let html = "A &amp; B &lt; C &gt; D &quot;E&quot; F&#39;s";
        let text = strip_html_tags(html);
        assert_eq!(text, "A & B < C > D \"E\" F's");
    }

    #[test]
    fn test_truncate_text_short() {
        let text = "short text";
        assert_eq!(truncate_text(text, 100), "short text");
    }

    #[test]
    fn test_truncate_text_long() {
        let text = "a".repeat(200);
        let result = truncate_text(&text, 100);
        assert!(result.contains("characters omitted"));
        assert!(result.len() < 200);
    }

    #[test]
    fn test_normalise_url_https() {
        let url = normalise_url("https://example.com").unwrap();
        assert_eq!(url, "https://example.com");
    }

    #[test]
    fn test_normalise_url_upgrade_http() {
        let url = normalise_url("http://example.com").unwrap();
        assert_eq!(url, "https://example.com");
    }

    #[test]
    fn test_normalise_url_add_scheme() {
        let url = normalise_url("example.com/path").unwrap();
        assert_eq!(url, "https://example.com/path");
    }

    #[test]
    fn test_normalise_url_empty() {
        assert!(normalise_url("").is_err());
    }

    #[test]
    fn test_normalise_url_too_long() {
        let long = "https://".to_string() + &"a".repeat(MAX_URL_LENGTH);
        assert!(normalise_url(&long).is_err());
    }

    #[test]
    fn test_normalise_url_rejects_embedded_credentials() {
        assert!(normalise_url("https://user@example.com").is_err());
        assert!(normalise_url("https://user:pass@example.com").is_err());
        assert!(normalise_url("user:pass@example.com").is_err());
    }

    #[test]
    fn test_redirect_policy_allows_same_host_and_www_variants() {
        assert!(is_permitted_redirect(
            "https://example.com/docs",
            "https://example.com/new-docs?x=1"
        ));
        assert!(is_permitted_redirect(
            "https://example.com/docs",
            "https://www.example.com/docs"
        ));
        assert!(is_permitted_redirect(
            "https://www.example.com/docs",
            "https://example.com/docs"
        ));
    }

    #[test]
    fn test_redirect_policy_rejects_cross_boundary_redirects() {
        assert!(!is_permitted_redirect(
            "https://example.com/docs",
            "https://evil.example.net/docs"
        ));
        assert!(!is_permitted_redirect(
            "https://example.com/docs",
            "http://example.com/docs"
        ));
        assert!(!is_permitted_redirect(
            "https://example.com/docs",
            "https://example.com:8443/docs"
        ));
        assert!(!is_permitted_redirect(
            "https://example.com/docs",
            "https://user@example.com/docs"
        ));
    }

    #[test]
    fn test_resolve_redirect_url_handles_relative_locations() {
        let resolved =
            resolve_redirect_url("https://example.com/a/b/page.html", "../target?q=1").unwrap();
        assert_eq!(resolved, "https://example.com/a/target?q=1");
    }

    #[test]
    fn test_proxy_url_prefers_scheme_specific_proxy() {
        let env = |name: &str| match name {
            "HTTPS_PROXY" => Some("http://secure-proxy.local:8080".to_string()),
            "HTTP_PROXY" => Some("http://plain-proxy.local:8080".to_string()),
            _ => None,
        };
        assert_eq!(
            proxy_url_from_env_for_url_with("https://example.com/path", env).as_deref(),
            Some("http://secure-proxy.local:8080")
        );
    }

    #[test]
    fn test_proxy_url_falls_back_to_all_proxy() {
        let env = |name: &str| match name {
            "ALL_PROXY" => Some("socks5://proxy.local:1080".to_string()),
            _ => None,
        };
        assert_eq!(
            proxy_url_from_env_for_url_with("https://example.com/path", env).as_deref(),
            Some("socks5://proxy.local:1080")
        );
    }

    #[test]
    fn test_proxy_url_honors_no_proxy_exact_suffix_and_wildcard() {
        let env = |name: &str| match name {
            "HTTPS_PROXY" => Some("http://proxy.local:8080".to_string()),
            "NO_PROXY" => Some("localhost,.internal.example,example.org:443".to_string()),
            _ => None,
        };
        assert!(proxy_url_from_env_for_url_with("https://localhost/status", env).is_none());
        assert!(proxy_url_from_env_for_url_with("https://api.internal.example/v1", env).is_none());
        assert!(proxy_url_from_env_for_url_with("https://example.org/path", env).is_none());

        let env = |name: &str| match name {
            "HTTPS_PROXY" => Some("http://proxy.local:8080".to_string()),
            "NO_PROXY" => Some("*".to_string()),
            _ => None,
        };
        assert!(proxy_url_from_env_for_url_with("https://example.com/path", env).is_none());
    }

    #[test]
    fn test_content_type_classification_matches_text_and_binary_boundaries() {
        assert_eq!(
            response_content_kind("text/html; charset=utf-8"),
            ResponseContentKind::Html
        );
        assert_eq!(
            response_content_kind("application/vnd.api+json"),
            ResponseContentKind::Json
        );
        assert_eq!(
            response_content_kind("application/xml"),
            ResponseContentKind::Text
        );
        assert_eq!(
            response_content_kind("application/pdf"),
            ResponseContentKind::Binary
        );
        assert_eq!(
            response_content_kind(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            ),
            ResponseContentKind::Binary
        );
    }

    #[test]
    fn test_extract_text_for_content_type_handles_html_case_insensitively() {
        let text = extract_text_for_content_type(
            "Text/HTML; charset=UTF-8",
            b"<html><body><script>hidden()</script><h1>Hello</h1></body></html>",
        );
        assert_eq!(text, "Hello");
    }

    #[test]
    fn test_extract_text_for_content_type_pretty_prints_json() {
        let text = extract_text_for_content_type("application/json", br#"{"b":2,"a":1}"#);
        assert_eq!(text, "{\n  \"a\": 1,\n  \"b\": 2\n}");
    }

    #[test]
    fn test_cache_put_get() {
        cache_put("https://test-cache.example.com", "cached content", 200);
        let result = cache_get("https://test-cache.example.com");
        assert!(result.is_some());
        let (content, status) = result.unwrap();
        assert_eq!(content, "cached content");
        assert_eq!(status, 200);
    }

    #[test]
    fn test_cache_miss() {
        let result = cache_get("https://never-cached.example.com");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_web_fetch_tool_basics() {
        let tool = WebFetchTool;
        assert_eq!(tool.name(), "WebFetch");
        assert!(tool.is_read_only(&json!({})));
        assert!(tool.is_concurrency_safe(&json!({})));

        let schema = tool.input_json_schema();
        assert!(schema["properties"]["url"].is_object());
        assert!(schema["properties"]["prompt"].is_object());
    }

    #[test]
    fn test_user_facing_name() {
        let tool = WebFetchTool;
        let input = json!({"url": "https://docs.rs/reqwest/latest"});
        let name = tool.user_facing_name(Some(&input));
        assert_eq!(name, "WebFetch(docs.rs)");
    }
}
