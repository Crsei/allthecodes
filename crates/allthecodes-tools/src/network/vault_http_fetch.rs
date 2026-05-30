use std::collections::BTreeMap;
use std::fs;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use base64::Engine as _;
use serde_json::{json, Value};
use url::Url;

use super::host_is_blocked;
use crate::common::{string_param, truncate_utf8_bytes, validate_enum};
use crate::tool::{
    PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult,
};
use allthecodes_types::message::AssistantMessage;

pub struct VaultHttpFetchTool;

const VAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const VAULT_HTTP_BODY_CAP_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct VaultCredential {
    pub(crate) header_name: String,
    pub(crate) header_value: String,
    pub(crate) scrub_markers: Vec<String>,
}

fn vault_auth_key(input: &Value) -> Option<&str> {
    string_param(input, "vault_auth_key").or_else(|| string_param(input, "credential_ref"))
}

fn validate_header_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        bail!("auth_header_name must not be empty");
    }
    reqwest::header::HeaderName::from_bytes(name.as_bytes())
        .with_context(|| format!("invalid header name: {name}"))?;
    Ok(name.to_ascii_lowercase())
}

fn normalize_auth_scheme(input: &Value, entry: Option<&Value>) -> Result<String> {
    let scheme = string_param(input, "auth_scheme")
        .or_else(|| entry.and_then(|value| value.get("auth_scheme").and_then(Value::as_str)))
        .or_else(|| entry.and_then(|value| value.get("type").and_then(Value::as_str)))
        .unwrap_or("bearer")
        .trim()
        .to_ascii_lowercase();
    match scheme.as_str() {
        "bearer" | "basic" | "custom" => Ok(scheme),
        _ => bail!("auth_scheme must be one of: bearer, basic, custom"),
    }
}

fn secret_from_vault_entry(entry: &Value) -> Option<String> {
    if let Some(secret) = entry.as_str() {
        return Some(secret.to_string());
    }
    for key in ["token", "secret", "value", "api_key"] {
        if let Some(secret) = entry.get(key).and_then(Value::as_str) {
            return Some(secret.to_string());
        }
    }
    None
}

pub(crate) fn secret_scrub_markers(
    header_name: &str,
    header_value: &str,
    secret: &str,
) -> Vec<String> {
    let mut markers = Vec::new();
    for value in [
        secret.to_string(),
        format!("Bearer {secret}"),
        format!("Basic {secret}"),
        base64::engine::general_purpose::STANDARD.encode(secret.as_bytes()),
        header_value.to_string(),
        format!("{header_name}: {header_value}"),
    ] {
        if !value.is_empty() && !markers.contains(&value) {
            markers.push(value);
        }
    }
    markers.sort_by_key(|value| std::cmp::Reverse(value.len()));
    markers
}

pub(crate) fn scrub_secret_markers(value: &str, markers: &[String]) -> String {
    let mut scrubbed = value.to_string();
    for marker in markers {
        if !marker.is_empty() {
            scrubbed = scrubbed.replace(marker, "[redacted]");
        }
    }
    scrubbed
}

pub(crate) fn credential_header(ref_name: &str, input: &Value) -> Result<Option<VaultCredential>> {
    let path = allthecodes_config::paths::credentials_path();
    if !path.exists() {
        return Ok(None);
    }
    let value: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    let Some(entry) = value.get("vault").and_then(|v| v.get(ref_name)) else {
        return Ok(None);
    };
    let Some(secret) = secret_from_vault_entry(entry) else {
        bail!("vault credential entry {ref_name} has no token/secret/value");
    };
    let scheme = normalize_auth_scheme(input, Some(entry))?;
    let header_name = if let Some(name) = string_param(input, "auth_header_name")
        .or_else(|| entry.get("auth_header_name").and_then(Value::as_str))
    {
        validate_header_name(name)?
    } else {
        "authorization".to_string()
    };
    let header_value = match scheme.as_str() {
        "bearer" => format!("Bearer {secret}"),
        "basic" => format!("Basic {secret}"),
        "custom" => secret.clone(),
        _ => unreachable!("auth scheme validated"),
    };
    let scrub_markers = secret_scrub_markers(&header_name, &header_value, &secret);
    Ok(Some(VaultCredential {
        header_name,
        header_value,
        scrub_markers,
    }))
}

fn redact_header(name: &str, value: &str, secret_markers: &[String]) -> String {
    let value = if matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization" | "cookie" | "set-cookie" | "x-api-key"
    ) {
        "[redacted]".to_string()
    } else {
        value.to_string()
    };
    scrub_secret_markers(&value, secret_markers)
}

pub(crate) fn resolve_vault_redirect_url(current_url: &str, location: &str) -> Result<Url> {
    let base = Url::parse(current_url).context("invalid redirect base URL")?;
    base.join(location)
        .context("invalid redirect Location header")
}

pub(crate) fn cap_and_scrub_body_bytes(bytes: &[u8], secret_markers: &[String]) -> (String, bool) {
    let take = bytes.len().min(VAULT_HTTP_BODY_CAP_BYTES);
    let capped = &bytes[..take];
    let mut truncated = bytes.len() > VAULT_HTTP_BODY_CAP_BYTES;
    let text = String::from_utf8_lossy(capped);
    let scrubbed = scrub_secret_markers(&text, secret_markers);
    let (preview, utf8_truncated) = truncate_utf8_bytes(&scrubbed, VAULT_HTTP_BODY_CAP_BYTES);
    truncated |= utf8_truncated;
    (preview, truncated)
}

pub(crate) fn vault_http_display_preview(
    status: u16,
    headers: &BTreeMap<String, String>,
    body_truncated: bool,
    redirect: Option<&Value>,
) -> String {
    let content_type = headers
        .get("content-type")
        .map(String::as_str)
        .unwrap_or("unknown");
    let content_length = headers
        .get("content-length")
        .map(String::as_str)
        .unwrap_or("unknown");
    let body_state = if body_truncated {
        "body truncated"
    } else {
        "body complete"
    };
    let redirect_state = match redirect {
        Some(value) if value.get("blocked_target").and_then(Value::as_bool) == Some(true) => {
            "redirect not followed; blocked target"
        }
        Some(_) => "redirect not followed",
        None => "no redirect",
    };
    format!(
        "VaultHttpFetch status={status}; content-type={content_type}; content-length={content_length}; {body_state}; {redirect_state}"
    )
}

#[async_trait]
impl Tool for VaultHttpFetchTool {
    fn name(&self) -> &str {
        "VaultHttpFetch"
    }

    async fn description(&self, _input: &Value) -> String {
        "Fetch public HTTPS URLs with optional allthecodes vault credentials, no redirect following, response caps, and secret scrubbing.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {"type": "string"},
                "method": {"type": "string", "enum": ["GET", "POST", "PUT", "PATCH", "DELETE"]},
                "headers": {"type": "object", "additionalProperties": {"type": "string"}},
                "body": {"type": "string"},
                "vault_auth_key": {"type": "string", "description": "Name of a credential under ~/.allthecodes/credentials.json vault."},
                "auth_scheme": {"type": "string", "enum": ["bearer", "basic", "custom"], "default": "bearer"},
                "auth_header_name": {"type": "string", "description": "Header to receive custom auth credentials; defaults to authorization."},
                "reason": {"type": "string", "description": "Why this authenticated fetch is needed."},
                "credential_ref": {"type": "string", "description": "Deprecated allthecodes compatibility alias for vault_auth_key."}
            },
            "required": ["url", "reason"]
        })
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let Some(raw_url) = string_param(input, "url") else {
            return ValidationResult::Error {
                message: "url is required".into(),
                error_code: 400,
            };
        };
        let Ok(url) = Url::parse(raw_url) else {
            return ValidationResult::Error {
                message: "url must be an absolute URL".into(),
                error_code: 400,
            };
        };
        if !url.username().is_empty() || url.password().is_some() {
            return ValidationResult::Error {
                message: "URL must not contain embedded credentials".into(),
                error_code: 400,
            };
        }
        if url.scheme() != "https" {
            return ValidationResult::Error {
                message: "VaultHttpFetch only allows HTTPS URLs".into(),
                error_code: 400,
            };
        }
        if host_is_blocked(&url) {
            return ValidationResult::Error {
                message:
                    "localhost, private, link-local, and unspecified network targets are blocked"
                        .into(),
                error_code: 400,
            };
        }
        if let Some(result) =
            validate_enum(input, "method", &["GET", "POST", "PUT", "PATCH", "DELETE"])
        {
            return result;
        }
        if let Some(result) = validate_enum(input, "auth_scheme", &["bearer", "basic", "custom"]) {
            return result;
        }
        if string_param(input, "reason").is_none() {
            return ValidationResult::Error {
                message: "reason is required".into(),
                error_code: 400,
            };
        }
        if string_param(input, "vault_auth_key").is_some()
            && string_param(input, "credential_ref").is_some()
        {
            return ValidationResult::Error {
                message: "use vault_auth_key or deprecated credential_ref, not both".into(),
                error_code: 400,
            };
        }
        if let Some(name) = string_param(input, "auth_header_name") {
            if let Err(err) = validate_header_name(name) {
                return ValidationResult::Error {
                    message: err.to_string(),
                    error_code: 400,
                };
            }
        }
        if input.get("body").is_some_and(|value| !value.is_string()) {
            return ValidationResult::Error {
                message: "body must be a string".into(),
                error_code: 400,
            };
        }
        if let Some(headers) = input.get("headers") {
            let Some(headers) = headers.as_object() else {
                return ValidationResult::Error {
                    message: "headers must be an object".into(),
                    error_code: 400,
                };
            };
            for (name, value) in headers {
                if validate_header_name(name).is_err() || !value.is_string() {
                    return ValidationResult::Error {
                        message: "headers must map valid header names to string values".into(),
                        error_code: 400,
                    };
                }
            }
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionResult {
        let host = string_param(input, "url")
            .and_then(|raw| Url::parse(raw).ok())
            .and_then(|url| url.host_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| "<unknown-host>".to_string());
        let key = vault_auth_key(input).unwrap_or("anonymous");
        let reason = string_param(input, "reason").unwrap_or("<missing reason>");
        PermissionResult::Ask {
            message: format!(
                "Allow VaultHttpFetch({key}@{host}) to request {}? Reason: {reason}",
                string_param(input, "url").unwrap_or("<missing url>")
            ),
        }
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let url = string_param(&input, "url").unwrap();
        let method = string_param(&input, "method").unwrap_or("GET");
        let client = reqwest::Client::builder()
            .timeout(VAULT_HTTP_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let mut request = client.request(method.parse()?, url);
        if let Some(headers) = input.get("headers").and_then(Value::as_object) {
            for (name, value) in headers {
                if let Some(value) = value.as_str() {
                    request = request.header(name, value);
                }
            }
        }
        let mut secret_markers = Vec::new();
        if let Some(vault_auth_key) = vault_auth_key(&input) {
            let Some(credential) = credential_header(vault_auth_key, &input)? else {
                bail!(
                    "vault_auth_key not found in allthecodes credentials vault: {vault_auth_key}"
                );
            };
            request = request.header(&credential.header_name, &credential.header_value);
            secret_markers = credential.scrub_markers;
        }
        if let Some(body) = input.get("body").and_then(Value::as_str) {
            request = request.body(body.to_string());
        }
        let mut response = request.send().await?;
        let status = response.status().as_u16();
        let redirect = if response.status().is_redirection() {
            response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .map(|location| {
                    let resolved = resolve_vault_redirect_url(url, location);
                    let blocked = resolved.as_ref().map(host_is_blocked).unwrap_or(true);
                    json!({
                        "location": scrub_secret_markers(location, &secret_markers),
                        "resolved_url": resolved
                            .as_ref()
                            .ok()
                            .map(|url| scrub_secret_markers(url.as_str(), &secret_markers)),
                        "blocked_target": blocked,
                        "followed": false,
                    })
                })
        } else {
            None
        };
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                let name = name.as_str().to_ascii_lowercase();
                if matches!(
                    name.as_str(),
                    "content-type"
                        | "content-length"
                        | "etag"
                        | "last-modified"
                        | "cache-control"
                        | "www-authenticate"
                        | "location"
                ) {
                    Some((
                        name.clone(),
                        redact_header(&name, value.to_str().unwrap_or("<binary>"), &secret_markers),
                    ))
                } else {
                    None
                }
            })
            .collect::<BTreeMap<_, _>>();
        let mut body = Vec::new();
        let mut body_truncated = false;
        while let Some(chunk) = response.chunk().await? {
            if body.len() + chunk.len() > VAULT_HTTP_BODY_CAP_BYTES {
                let remaining = VAULT_HTTP_BODY_CAP_BYTES.saturating_sub(body.len());
                body.extend_from_slice(&chunk[..remaining]);
                body_truncated = true;
                break;
            }
            body.extend_from_slice(&chunk);
        }
        let (preview, scrub_truncated) = cap_and_scrub_body_bytes(&body, &secret_markers);
        body_truncated |= scrub_truncated;
        let display_preview =
            vault_http_display_preview(status, &headers, body_truncated, redirect.as_ref());
        Ok(ToolResult {
            data: json!({
                "status": status,
                "headers": headers,
                "body_preview": preview,
                "body_truncated": body_truncated,
                "body_cap_bytes": VAULT_HTTP_BODY_CAP_BYTES,
                "redirect": redirect,
            }),
            display_preview: Some(display_preview),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Fetch public HTTPS resources with optional allthecodes vault credentials. Provide a reason, use vault_auth_key for credentials, never target localhost/private networks, and expect redirects not to be followed."
            .into()
    }

    fn to_auto_classifier_input(&self, input: &Value) -> Value {
        json!({
            "url": input.get("url"),
            "method": input.get("method"),
            "vault_auth_key": vault_auth_key(input),
            "reason": input.get("reason"),
        })
    }
}
