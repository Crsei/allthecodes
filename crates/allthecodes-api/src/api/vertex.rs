//! GCP Vertex AI provider 鈥?routes Claude requests to Google-managed Claude
//! endpoints.
//!
//! Like Bedrock, this is an adaptation layer, not a separate product. Vertex
//! natively streams SSE in the same format as the Anthropic API, so this
//! module can reuse the existing SSE parser directly.
//!
//! # Endpoint
//!
//! `POST https://{region}-aiplatform.googleapis.com/v1/projects/{project}
//!   /locations/{region}/publishers/anthropic/models/{model}:streamRawPredict`
//!
//! # Authentication
//!
//! Access token resolved from first of:
//! 1. `ALLTHECODES_VERTEX_ACCESS_TOKEN` (explicit override, highest priority)
//! 2. `GOOGLE_OAUTH_ACCESS_TOKEN`
//! 3. `GOOGLE_APPLICATION_CREDENTIALS` service-account JSON via JWT bearer exchange
//! 4. `gcloud auth application-default print-access-token` subprocess
//!
//! # Project / region resolution
//!
//! Project ID (first non-empty wins):
//! - `ANTHROPIC_VERTEX_PROJECT_ID`
//! - `GOOGLE_CLOUD_PROJECT`
//! - `GCLOUD_PROJECT`
//!
//! Region (first non-empty wins):
//! - `CLOUD_ML_REGION`
//! - default `us-east5`

use std::fs;
use std::path::PathBuf;
use std::pin::Pin;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use futures::Stream;
use serde_json::{json, Value};

use crate::api::client::{parse_sse_byte_stream, strip_anthropic_cache_fields, MessagesRequest};
use crate::api::retry::categorize_api_error;
use allthecodes_types::message::StreamEvent;
use allthecodes_types::models::to_vertex_model_id;

pub const VERTEX_ANTHROPIC_VERSION: &str = "vertex-2023-10-16";
pub const DEFAULT_VERTEX_REGION: &str = "us-east5";
const GOOGLE_CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const DEFAULT_GOOGLE_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
const JWT_BEARER_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

/// Model prefix -> env var for Vertex region overrides.
///
/// This mirrors claude-code-bun's per-model override table. More specific
/// prefixes must appear before less specific prefixes.
const VERTEX_REGION_OVERRIDES: &[(&str, &str)] = &[
    ("claude-haiku-4-5", "VERTEX_REGION_CLAUDE_HAIKU_4_5"),
    ("claude-3-5-haiku", "VERTEX_REGION_CLAUDE_3_5_HAIKU"),
    ("claude-3-5-sonnet", "VERTEX_REGION_CLAUDE_3_5_SONNET"),
    ("claude-3-7-sonnet", "VERTEX_REGION_CLAUDE_3_7_SONNET"),
    ("claude-opus-4-1", "VERTEX_REGION_CLAUDE_4_1_OPUS"),
    ("claude-opus-4", "VERTEX_REGION_CLAUDE_4_0_OPUS"),
    ("claude-sonnet-4-6", "VERTEX_REGION_CLAUDE_4_6_SONNET"),
    ("claude-sonnet-4-5", "VERTEX_REGION_CLAUDE_4_5_SONNET"),
    ("claude-sonnet-4", "VERTEX_REGION_CLAUDE_4_0_SONNET"),
];

/// Pre-obtained OAuth access token used for Vertex calls.
#[derive(Debug, Clone)]
pub struct VertexAccessToken(pub String);

impl VertexAccessToken {
    /// Resolve an access token from the environment, service-account JSON, or
    /// `gcloud` CLI.
    ///
    /// Returns `None` if no source succeeds.
    pub fn from_env_or_gcloud() -> Option<Self> {
        if let Ok(t) = std::env::var("ALLTHECODES_VERTEX_ACCESS_TOKEN") {
            if !t.is_empty() {
                return Some(Self(t));
            }
        }
        if let Ok(t) = std::env::var("GOOGLE_OAUTH_ACCESS_TOKEN") {
            if !t.is_empty() {
                return Some(Self(t));
            }
        }
        if let Some(t) = fetch_service_account_token() {
            return Some(Self(t));
        }
        fetch_gcloud_token().map(Self)
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ServiceAccountCredentials {
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    client_email: Option<String>,
    #[serde(default)]
    private_key: Option<String>,
    #[serde(default)]
    private_key_id: Option<String>,
    #[serde(default)]
    token_uri: Option<String>,
}

impl ServiceAccountCredentials {
    fn is_service_account(&self) -> bool {
        self.r#type.as_deref() == Some("service_account")
            || (self.client_email.is_some() && self.private_key.is_some())
    }

    fn client_email(&self) -> Result<&str> {
        self.client_email
            .as_deref()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| anyhow!("service-account JSON is missing client_email"))
    }

    fn private_key(&self) -> Result<&str> {
        self.private_key
            .as_deref()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| anyhow!("service-account JSON is missing private_key"))
    }

    fn token_uri(&self) -> &str {
        self.token_uri
            .as_deref()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or(DEFAULT_GOOGLE_TOKEN_URI)
    }
}

/// Exchange `GOOGLE_APPLICATION_CREDENTIALS` service-account credentials for a
/// Google OAuth access token.
fn fetch_service_account_token() -> Option<String> {
    let credentials = match read_service_account_credentials_from_env() {
        Ok(Some(credentials)) => credentials,
        Ok(None) => return None,
        Err(error) => {
            tracing::warn!(%error, "failed to read Vertex service-account credentials");
            return None;
        }
    };

    match exchange_service_account_credentials_blocking(credentials) {
        Ok(token) => Some(token),
        Err(error) => {
            tracing::warn!(%error, "failed to exchange Vertex service-account JWT");
            None
        }
    }
}

fn service_account_credentials_path_from_env() -> Option<PathBuf> {
    for var in [
        "GOOGLE_APPLICATION_CREDENTIALS",
        "google_application_credentials",
    ] {
        if let Ok(path) = std::env::var(var) {
            if !path.trim().is_empty() {
                return Some(PathBuf::from(path));
            }
        }
    }
    None
}

fn read_service_account_credentials_from_env() -> Result<Option<ServiceAccountCredentials>> {
    let Some(path) = service_account_credentials_path_from_env() else {
        return Ok(None);
    };
    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let credentials: ServiceAccountCredentials = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    if credentials.is_service_account() {
        Ok(Some(credentials))
    } else {
        Ok(None)
    }
}

fn exchange_service_account_credentials_blocking(
    credentials: ServiceAccountCredentials,
) -> Result<String> {
    let assertion = build_service_account_jwt(&credentials, current_unix_timestamp()?)?;
    let token_uri = credentials.token_uri().to_string();

    thread::spawn(move || -> Result<String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to create Vertex service-account exchange runtime")?;
        runtime.block_on(async move {
            let http = reqwest::Client::new();
            exchange_service_account_jwt(&http, &token_uri, &assertion).await
        })
    })
    .join()
    .map_err(|_| anyhow!("Vertex service-account exchange thread panicked"))?
}

fn current_unix_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_secs())
}

fn build_service_account_jwt(
    credentials: &ServiceAccountCredentials,
    issued_at: u64,
) -> Result<String> {
    build_service_account_jwt_with_signer(credentials, issued_at, sign_rs256)
}

fn build_service_account_jwt_with_signer<F>(
    credentials: &ServiceAccountCredentials,
    issued_at: u64,
    signer: F,
) -> Result<String>
where
    F: FnOnce(&str, &str) -> Result<String>,
{
    let mut header = json!({
        "alg": "RS256",
        "typ": "JWT",
    });
    if let Some(kid) = credentials
        .private_key_id
        .as_deref()
        .filter(|v| !v.trim().is_empty())
    {
        header["kid"] = Value::String(kid.to_string());
    }

    let claims = json!({
        "iss": credentials.client_email()?,
        "scope": GOOGLE_CLOUD_PLATFORM_SCOPE,
        "aud": credentials.token_uri(),
        "iat": issued_at,
        "exp": issued_at + 3600,
    });

    let signing_input = format!("{}.{}", base64url_json(&header)?, base64url_json(&claims)?);
    let signature = signer(&signing_input, credentials.private_key()?)?;
    Ok(format!("{signing_input}.{signature}"))
}

fn base64url_json(value: &Value) -> Result<String> {
    let bytes = serde_json::to_vec(value).context("failed to serialize JWT JSON")?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn sign_rs256(signing_input: &str, private_key_pem: &str) -> Result<String> {
    let der = decode_private_key_pem(private_key_pem)?;
    let key_pair = ring::signature::RsaKeyPair::from_pkcs8(&der)
        .or_else(|_| ring::signature::RsaKeyPair::from_der(&der))
        .map_err(|_| anyhow!("failed to parse service-account RSA private key"))?;
    let rng = ring::rand::SystemRandom::new();
    let mut signature = vec![0; key_pair.public().modulus_len()];
    key_pair
        .sign(
            &ring::signature::RSA_PKCS1_SHA256,
            &rng,
            signing_input.as_bytes(),
            &mut signature,
        )
        .map_err(|_| anyhow!("failed to sign service-account JWT"))?;
    Ok(URL_SAFE_NO_PAD.encode(signature))
}

fn decode_private_key_pem(pem: &str) -> Result<Vec<u8>> {
    for label in ["PRIVATE KEY", "RSA PRIVATE KEY"] {
        if let Some(der) = decode_pem_block(pem, label)? {
            return Ok(der);
        }
    }
    bail!("service-account private_key is not a supported PEM private key")
}

fn decode_pem_block(pem: &str, label: &str) -> Result<Option<Vec<u8>>> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let Some(begin_index) = pem.find(&begin) else {
        return Ok(None);
    };
    let body_start = begin_index + begin.len();
    let Some(end_offset) = pem[body_start..].find(&end) else {
        bail!("service-account private_key PEM is missing END {label} marker");
    };
    let body = &pem[body_start..body_start + end_offset];
    let encoded: String = body.lines().map(str::trim).collect();
    let der = STANDARD
        .decode(encoded.as_bytes())
        .context("failed to decode service-account private_key PEM")?;
    Ok(Some(der))
}

async fn exchange_service_account_jwt(
    http: &reqwest::Client,
    token_uri: &str,
    assertion: &str,
) -> Result<String> {
    let response = http
        .post(token_uri)
        .form(&[
            ("grant_type", JWT_BEARER_GRANT_TYPE),
            ("assertion", assertion),
        ])
        .send()
        .await
        .context("failed to send Vertex service-account token exchange")?;
    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| String::from("(failed to read token response body)"));
    if !status.is_success() {
        bail!(
            "Vertex service-account token exchange failed (HTTP {}): {}",
            status.as_u16(),
            body
        );
    }
    parse_service_account_token_response(&body)
}

fn parse_service_account_token_response(body: &str) -> Result<String> {
    let value: Value =
        serde_json::from_str(body).context("failed to parse Vertex token response JSON")?;
    if let Some(token) = value
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    {
        return Ok(token.to_string());
    }

    let error = value
        .get("error_description")
        .or_else(|| value.get("error"))
        .and_then(Value::as_str)
        .unwrap_or("missing access_token");
    bail!("Vertex token response did not include access_token: {error}")
}

/// Invoke `gcloud auth application-default print-access-token` to obtain an
/// access token via Application Default Credentials.
fn fetch_gcloud_token() -> Option<String> {
    let output = std::process::Command::new("gcloud")
        .args([
            "auth",
            "application-default",
            "print-access-token",
            "--quiet",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

/// Resolve the project ID from environment.
pub fn resolve_project_id() -> Option<String> {
    for var in [
        "ANTHROPIC_VERTEX_PROJECT_ID",
        "GOOGLE_CLOUD_PROJECT",
        "GCLOUD_PROJECT",
    ] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// Resolve the region from environment, defaulting to `us-east5`.
pub fn resolve_region() -> String {
    resolve_region_from_value(std::env::var("CLOUD_ML_REGION").ok())
}

fn resolve_region_from_value(value: Option<String>) -> String {
    value
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_VERTEX_REGION.to_string())
}

/// Resolve the Vertex region for a model, honoring per-model overrides.
///
/// The default region is normally `CLOUD_ML_REGION` or `us-east5`; callers can
/// pass a different already-resolved default for constructed clients.
pub fn resolve_region_for_model_with_default(model: Option<&str>, default_region: &str) -> String {
    if let Some(model) = model {
        for (prefix, env_var) in VERTEX_REGION_OVERRIDES {
            if model.starts_with(prefix) {
                return std::env::var(env_var)
                    .ok()
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| default_region.to_string());
            }
        }
    }
    default_region.to_string()
}

/// Resolve the Vertex region for a model using process environment defaults.
pub fn resolve_region_for_model(model: Option<&str>) -> String {
    let default_region = resolve_region();
    resolve_region_for_model_with_default(model, &default_region)
}

/// Build the Vertex `:streamRawPredict` URL for a given model.
pub fn build_stream_url(region: &str, project_id: &str, model_id: &str) -> String {
    format!(
        "https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}/publishers/anthropic/models/{model}:streamRawPredict",
        region = region,
        project = project_id,
        model = model_id,
    )
}

/// Build the Vertex Anthropic `count-tokens:rawPredict` URL.
pub fn build_count_tokens_url(region: &str, project_id: &str) -> String {
    format!(
        "https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}/publishers/anthropic/models/count-tokens:rawPredict",
        region = region,
        project = project_id,
    )
}

/// Convert a `MessagesRequest` into the Vertex-specific JSON body.
///
/// Same as Bedrock: strip `model` and `stream`; add `anthropic_version`.
fn to_vertex_body(request: &MessagesRequest) -> Result<Vec<u8>> {
    let mut messages = Value::Array(request.messages.clone());
    strip_anthropic_cache_fields(&mut messages);
    let mut body = json!({
        "anthropic_version": VERTEX_ANTHROPIC_VERSION,
        "max_tokens": request.max_tokens,
        "messages": messages.as_array().cloned().unwrap_or_default(),
    });
    if let Some(system) = &request.system {
        let mut value = Value::Array(system.clone());
        strip_anthropic_cache_fields(&mut value);
        body["system"] = value;
    }
    if let Some(tools) = &request.tools {
        let mut value = Value::Array(tools.clone());
        strip_anthropic_cache_fields(&mut value);
        body["tools"] = value;
    }
    if let Some(thinking) = &request.thinking {
        body["thinking"] = thinking.clone();
    }
    if let Some(tool_choice) = &request.tool_choice {
        body["tool_choice"] = tool_choice.clone();
    }
    serde_json::to_vec(&body).context("failed to serialize Vertex request body")
}

/// Convert a `MessagesRequest` into the Vertex Anthropic Count Tokens body.
///
/// The count endpoint is a rawPredict model endpoint whose request contains
/// the target Anthropic model ID plus the input-bearing fields. Generation-only
/// fields from the streaming call are omitted.
fn to_vertex_count_tokens_body(request: &MessagesRequest) -> Result<Vec<u8>> {
    let mut messages = Value::Array(request.messages.clone());
    strip_anthropic_cache_fields(&mut messages);
    let mut body = json!({
        "model": to_vertex_model_id(&request.model),
        "messages": messages.as_array().cloned().unwrap_or_default(),
    });
    if let Some(system) = &request.system {
        let mut value = Value::Array(system.clone());
        strip_anthropic_cache_fields(&mut value);
        body["system"] = value;
    }
    if let Some(tools) = &request.tools {
        let mut value = Value::Array(tools.clone());
        strip_anthropic_cache_fields(&mut value);
        body["tools"] = value;
    }
    if let Some(thinking) = &request.thinking {
        body["thinking"] = thinking.clone();
    }
    if let Some(tool_choice) = &request.tool_choice {
        body["tool_choice"] = tool_choice.clone();
    }
    serde_json::to_vec(&body).context("failed to serialize Vertex Count Tokens body")
}

pub(crate) async fn vertex_count_tokens(
    http: &reqwest::Client,
    project_id: &str,
    region: &str,
    access_token: &VertexAccessToken,
    request: &MessagesRequest,
) -> Result<u64> {
    #[derive(serde::Deserialize)]
    struct CountTokensResponse {
        input_tokens: u64,
    }

    let region = resolve_region_for_model_with_default(Some(&request.model), region);
    let url = build_count_tokens_url(&region, project_id);
    let body = to_vertex_count_tokens_body(request)?;

    let response = http
        .post(&url)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", access_token.0))
        .body(body)
        .send()
        .await
        .context("failed to send Vertex Count Tokens request")?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("(failed to read Vertex Count Tokens body)"));
        let category = categorize_api_error(status.as_u16(), &error_body);
        bail!(
            "Vertex Count Tokens error (HTTP {}): {:?} - {}",
            status.as_u16(),
            category,
            error_body
        );
    }

    let parsed: CountTokensResponse = response
        .json()
        .await
        .context("failed to parse Vertex Count Tokens response")?;
    Ok(parsed.input_tokens)
}

/// Vertex stream provider (implements `StreamProvider`).
pub struct VertexStreamProvider {
    pub region: String,
    pub project_id: String,
    pub access_token: VertexAccessToken,
}

#[async_trait::async_trait]
impl crate::api::stream_provider::StreamProvider for VertexStreamProvider {
    async fn stream(
        &self,
        http: &reqwest::Client,
        request: &MessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let vertex_model = to_vertex_model_id(&request.model);
        let region = resolve_region_for_model_with_default(Some(&request.model), &self.region);
        let url = build_stream_url(&region, &self.project_id, &vertex_model);
        let body = to_vertex_body(request)?;

        let response = http
            .post(&url)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", self.access_token.0))
            .body(body)
            .send()
            .await
            .context("failed to send Vertex streamRawPredict request")?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| String::from("(failed to read error body)"));
            let category = categorize_api_error(status.as_u16(), &error_body);
            bail!(
                "Vertex AI error (HTTP {}): {:?} 鈥?{}",
                status.as_u16(),
                category,
                error_body
            );
        }

        let byte_stream = response.bytes_stream();
        let sse_stream = parse_sse_byte_stream(byte_stream);
        Ok(Box::pin(sse_stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQClEWq9LY0s20JT
F8m3RvKPyx2YIo1HVqnrglG+9bBX1xwelVOHpAydmmGravqshS6efq5FlScdYYNp
oZgKC9wN4DmPWRwCC1688W660zqMB4CMKs3U8Q+NpRDT8LfqaULfP9ZawSnWfWpS
t3DIMSwzu1HKrOFyLAeaYf3grnnK0rRV9pWmgD7/Hp8eGCmDHaCGkYhFlhEEkg1G
OxH5wT6oXRRL581s2PhOB2iB4LEffx5aQGeMC2NTnZ1RKh2IUpuYnfdCNWJsipHc
o7j4rsAYlDuMLPaWA42wGRLbuOZ8QS3Qv0ePf6BCxP9b/dLgkEtLrsssLXnIcKN7
KCbTaFTNAgMBAAECggEAHTlN6zO9Foe0AJeJziGosIX+lZBqcEqa1zfxhowjXg3W
q+B2kyFbXWy3ZXyRaElE9WkKrAWJ0QUSWbly/DZYzXkY37TgRUljirJ4zuk2KJPs
cYRjgBN0lDh41/j6aq0bmoBIEDW6FUALep0BAeRYxcjgZHBCkq7SYsX+B1EEfYCS
6Wpt44Kqnyrr0bDVSLutDyohewO8R6Euml1x1U6k3HhZtMYGclUbESdur3rdVEXx
ED9/HO71/rPkesQFC5g/zsNB3VAsgfL7nSDEHJ81SYz+m5rEFQuE3XdzukBCmRP3
+BXFdbJiCwBslzHx3y7xdtEtTaYbJZ2aDaxx8YeJ2QKBgQDY8wFZjw8Z6X+g0mlY
sORNOIx6fqxjKSKuEBAj19m4w0upr2qnBEEphOyZMcQNWx1hg1vh0UoR+CvoyR4f
azw9akRiGjDpRhwK4+doYRIce5/FtKg0fUazSIRiFxUsivGt1reCbxCThsebzT2K
eiHjM59p7x1UsRRYxZH1rnX55QKBgQDCx7tnjCQwbGFXkCNExpM9Xat1+EO+CGmu
C1XeDEV6TDu/Un+gmcWb2H/tCk0VuSgKAM6ScJSsZOJog0rI4HdeJGXTQM4jxZPi
oKBxah1qheL3PlU2VSKMAF7mfSRtbHPJx1g63tpRfjTgjTNJ3rt7pzvrGLnhVCEs
gOYFgLigyQKBgQCCjztwujdMUMkN75KWXV4HWtCvpyv3QPot5lzQNUZBesY+B+MX
P+g3JFd+mgRyRTMZTAQvkdQRnFhQbzhBkDdgfmNI5pooyJh3tU+98S0FFyC/ykiv
zfSOUEXbAikr9TIce+tUA6LmJWZazNkMTRO3t4loJw5vuWGVStDcGXHGQQKBgCT+
SDKHZEwqGWbHAlvKlyZdhvYV28/YyzF6B6nvjLaIigRxR7oZ2nUZ7ln3zeIlU1xr
ANDBPwtq8bFF1ktGjoU7xncT5NLYcJjnRvGjZMjZetzYYti53KDYZS3DcMqzgV4+
VRyBPNejb6mCR85s1hDLF080WAFauB46sPU0mFw5AoGAboZqKdvrYiaibM90G8zC
2WXiNz9a8zJv4acRmhBBtcbgq/TqAflhjzNfUHLiY81BAyliIMCRIBmYaqJDTpPc
aM0cnYVle4nyuGi3M6aECuC6ggfLfXOQ3yGAmE3DKg2bgcmJag2cOT6fTRZemThD
0fuV0xXNiHf3sCBzRMp5gfc=
-----END PRIVATE KEY-----"#;

    fn decode_jwt_json(segment: &str) -> Value {
        let bytes = URL_SAFE_NO_PAD.decode(segment.as_bytes()).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn region_defaults_to_us_east5() {
        assert_eq!(resolve_region_from_value(None), "us-east5");
    }

    #[test]
    fn region_uses_cloud_ml_region() {
        assert_eq!(
            resolve_region_from_value(Some("europe-west4".to_string())),
            "europe-west4"
        );
    }

    #[test]
    fn region_for_model_uses_specific_override() {
        let saved = std::env::var("VERTEX_REGION_CLAUDE_HAIKU_4_5").ok();
        std::env::set_var("VERTEX_REGION_CLAUDE_HAIKU_4_5", "us-central1");

        assert_eq!(
            resolve_region_for_model_with_default(Some("claude-haiku-4-5-20251001"), "us-east5"),
            "us-central1"
        );

        match saved {
            Some(v) => std::env::set_var("VERTEX_REGION_CLAUDE_HAIKU_4_5", v),
            None => std::env::remove_var("VERTEX_REGION_CLAUDE_HAIKU_4_5"),
        }
    }

    #[test]
    fn region_for_model_falls_back_to_default_region() {
        let saved = std::env::var("VERTEX_REGION_CLAUDE_4_5_SONNET").ok();
        std::env::remove_var("VERTEX_REGION_CLAUDE_4_5_SONNET");

        assert_eq!(
            resolve_region_for_model_with_default(
                Some("claude-sonnet-4-5-20250929"),
                "europe-west4"
            ),
            "europe-west4"
        );

        if let Some(v) = saved {
            std::env::set_var("VERTEX_REGION_CLAUDE_4_5_SONNET", v);
        }
    }

    #[test]
    fn url_construction() {
        let url = build_stream_url("us-east5", "my-proj", "claude-sonnet-4-5@20250929");
        assert_eq!(
            url,
            "https://us-east5-aiplatform.googleapis.com/v1/projects/my-proj/locations/us-east5/publishers/anthropic/models/claude-sonnet-4-5@20250929:streamRawPredict"
        );
    }

    #[test]
    fn count_tokens_url_construction() {
        let url = build_count_tokens_url("us-east5", "my-proj");
        assert_eq!(
            url,
            "https://us-east5-aiplatform.googleapis.com/v1/projects/my-proj/locations/us-east5/publishers/anthropic/models/count-tokens:rawPredict"
        );
    }

    #[test]
    fn project_id_priority() {
        let saved_an = std::env::var("ANTHROPIC_VERTEX_PROJECT_ID").ok();
        let saved_gc = std::env::var("GOOGLE_CLOUD_PROJECT").ok();
        let saved_gcl = std::env::var("GCLOUD_PROJECT").ok();
        std::env::remove_var("ANTHROPIC_VERTEX_PROJECT_ID");
        std::env::remove_var("GOOGLE_CLOUD_PROJECT");
        std::env::remove_var("GCLOUD_PROJECT");

        // None set 鈫?None
        assert_eq!(resolve_project_id(), None);

        // Only fallback set
        std::env::set_var("GCLOUD_PROJECT", "gcloud-proj");
        assert_eq!(resolve_project_id(), Some("gcloud-proj".to_string()));

        // Higher precedence wins
        std::env::set_var("GOOGLE_CLOUD_PROJECT", "gcp-proj");
        assert_eq!(resolve_project_id(), Some("gcp-proj".to_string()));

        std::env::set_var("ANTHROPIC_VERTEX_PROJECT_ID", "ant-proj");
        assert_eq!(resolve_project_id(), Some("ant-proj".to_string()));

        // restore
        std::env::remove_var("ANTHROPIC_VERTEX_PROJECT_ID");
        std::env::remove_var("GOOGLE_CLOUD_PROJECT");
        std::env::remove_var("GCLOUD_PROJECT");
        if let Some(v) = saved_an {
            std::env::set_var("ANTHROPIC_VERTEX_PROJECT_ID", v);
        }
        if let Some(v) = saved_gc {
            std::env::set_var("GOOGLE_CLOUD_PROJECT", v);
        }
        if let Some(v) = saved_gcl {
            std::env::set_var("GCLOUD_PROJECT", v);
        }
    }

    #[test]
    fn body_strips_stream_and_model_adds_anthropic_version() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-5-20250929".to_string(),
            messages: vec![json!({"role":"user","content":"hi"})],
            system: None,
            max_tokens: 256,
            tools: None,
            stream: true,
            metadata: None,
            service_tier: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
            context_management: None,
            thinking: None,
            output_config: None,
            tool_choice: None,
            reasoning_effort: None,
            advisor_model: None,
        };
        let raw = to_vertex_body(&req).unwrap();
        let v: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(v["anthropic_version"], VERTEX_ANTHROPIC_VERSION);
        assert_eq!(v["max_tokens"], 256);
        assert!(v.get("model").is_none());
        assert!(v.get("stream").is_none());
    }

    #[test]
    fn count_tokens_body_uses_model_and_strips_generation_fields() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-5-20250929".to_string(),
            messages: vec![json!({"role":"user","content":"hi"})],
            system: Some(vec![json!({"type":"text","text":"Be brief."})]),
            max_tokens: 256,
            tools: Some(vec![json!({
                "name": "Read",
                "description": "",
                "input_schema": {"type": "object"}
            })]),
            stream: true,
            metadata: None,
            service_tier: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
            context_management: None,
            thinking: Some(json!({"type":"enabled","budget_tokens":128})),
            output_config: None,
            tool_choice: None,
            reasoning_effort: None,
            advisor_model: Some("advisor".to_string()),
        };

        let raw = to_vertex_count_tokens_body(&req).unwrap();
        let v: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(v["model"], "claude-sonnet-4-5@20250929");
        assert_eq!(v["messages"][0]["content"], "hi");
        assert_eq!(v["system"][0]["text"], "Be brief.");
        assert!(v.get("tools").is_some());
        assert!(v.get("thinking").is_some());
        assert!(v.get("anthropic_version").is_none());
        assert!(v.get("max_tokens").is_none());
        assert!(v.get("stream").is_none());
        assert!(v.get("advisor_model").is_none());
    }

    #[test]
    fn service_account_jwt_contains_google_claims() {
        let credentials = ServiceAccountCredentials {
            r#type: Some("service_account".to_string()),
            client_email: Some("bot@example.iam.gserviceaccount.com".to_string()),
            private_key: Some(TEST_PRIVATE_KEY.to_string()),
            private_key_id: Some("kid-123".to_string()),
            token_uri: None,
        };

        let jwt = build_service_account_jwt(&credentials, 1_700_000_000).unwrap();
        let parts = jwt.split('.').collect::<Vec<_>>();
        assert_eq!(parts.len(), 3);

        let header = decode_jwt_json(parts[0]);
        assert_eq!(header["alg"], "RS256");
        assert_eq!(header["typ"], "JWT");
        assert_eq!(header["kid"], "kid-123");

        let claims = decode_jwt_json(parts[1]);
        assert_eq!(claims["iss"], "bot@example.iam.gserviceaccount.com");
        assert_eq!(claims["scope"], GOOGLE_CLOUD_PLATFORM_SCOPE);
        assert_eq!(claims["aud"], DEFAULT_GOOGLE_TOKEN_URI);
        assert_eq!(claims["iat"], 1_700_000_000u64);
        assert_eq!(claims["exp"], 1_700_003_600u64);
        assert!(!parts[2].is_empty());
    }

    #[test]
    fn service_account_jwt_allows_test_signer() {
        let credentials = ServiceAccountCredentials {
            r#type: Some("service_account".to_string()),
            client_email: Some("bot@example.iam.gserviceaccount.com".to_string()),
            private_key: Some("fake-key".to_string()),
            private_key_id: None,
            token_uri: Some("https://tokens.example.test".to_string()),
        };

        let jwt = build_service_account_jwt_with_signer(&credentials, 42, |input, key| {
            assert_eq!(key, "fake-key");
            assert!(input.contains('.'));
            Ok(URL_SAFE_NO_PAD.encode("signature"))
        })
        .unwrap();

        let parts = jwt.split('.').collect::<Vec<_>>();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[2], URL_SAFE_NO_PAD.encode("signature"));
        let claims = decode_jwt_json(parts[1]);
        assert_eq!(claims["aud"], "https://tokens.example.test");
    }

    #[test]
    fn service_account_parser_ignores_authorized_user_adc() {
        let credentials: ServiceAccountCredentials =
            serde_json::from_str(r#"{"type":"authorized_user","client_id":"abc"}"#).unwrap();
        assert!(!credentials.is_service_account());
    }

    #[test]
    fn service_account_token_response_requires_access_token() {
        assert_eq!(
            parse_service_account_token_response(r#"{"access_token":"ya29.token"}"#).unwrap(),
            "ya29.token"
        );
        let err = parse_service_account_token_response(r#"{"error":"invalid_grant"}"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid_grant"));
    }

    #[test]
    fn access_token_env_var_priority() {
        let saved_cc = std::env::var("ALLTHECODES_VERTEX_ACCESS_TOKEN").ok();
        let saved_go = std::env::var("GOOGLE_OAUTH_ACCESS_TOKEN").ok();
        std::env::set_var("ALLTHECODES_VERTEX_ACCESS_TOKEN", "cc-token");
        std::env::set_var("GOOGLE_OAUTH_ACCESS_TOKEN", "go-token");

        let t = VertexAccessToken::from_env_or_gcloud().unwrap();
        assert_eq!(t.0, "cc-token");

        std::env::remove_var("ALLTHECODES_VERTEX_ACCESS_TOKEN");
        let t = VertexAccessToken::from_env_or_gcloud().unwrap();
        assert_eq!(t.0, "go-token");

        std::env::remove_var("GOOGLE_OAUTH_ACCESS_TOKEN");
        if let Some(v) = saved_cc {
            std::env::set_var("ALLTHECODES_VERTEX_ACCESS_TOKEN", v);
        }
        if let Some(v) = saved_go {
            std::env::set_var("GOOGLE_OAUTH_ACCESS_TOKEN", v);
        }
    }
}
