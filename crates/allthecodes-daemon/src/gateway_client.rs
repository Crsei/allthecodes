//! Local client for the loopback remote-control gateway.
//!
//! The client intentionally talks to the daemon's loopback HTTP API for live
//! gateway operations.

use std::time::Duration;

use allthecodes_gateway::{
    AdapterProvider, AdapterStatus, AdapterTestMessage, GatewayChannelConfigSnapshot,
    GatewayChannelsConfigResponse, GatewayDiagnostic, OutputReadBatch, RunEvent, RunId, RunMeta,
};
use anyhow::Result;
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::process_state::{self, DaemonStatusSnapshot};

const TOKEN_HEADER: &str = "x-allthecodes-daemon-token";

#[derive(Debug, Clone)]
pub enum LocalGatewayDaemonStatus {
    Running {
        pid: u32,
        base_url: String,
        health_url: String,
        ready_url: Option<String>,
        binary_version: Option<String>,
        binary_path: Option<String>,
        log_path: Option<String>,
        identity: String,
    },
    Stale {
        pid: u32,
        binary_version: Option<String>,
        log_path: Option<String>,
        identity: String,
    },
    Stopped,
}

#[derive(Debug, Clone)]
pub struct LocalGatewayClient {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayCapabilitiesSnapshot {
    pub version: String,
    pub auth_mode: Value,
    pub supports_steer: bool,
    pub max_running: usize,
    pub max_queued: usize,
    pub endpoints: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayRunActionResponse {
    pub run_id: RunId,
    pub status: allthecodes_gateway::RunStatus,
    pub action: Value,
    #[serde(default)]
    pub diagnostic: Option<GatewayDiagnostic>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayRunTimeline {
    pub events: Vec<RunEvent>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayAdapters {
    pub adapters: Vec<AdapterStatus>,
}

impl LocalGatewayClient {
    pub fn daemon_status() -> Result<LocalGatewayDaemonStatus> {
        Ok(match process_state::status_snapshot()? {
            DaemonStatusSnapshot::Running(state) => LocalGatewayDaemonStatus::Running {
                identity: process_state::process_matches_record(
                    state.pid,
                    state.process_start_key.as_deref(),
                )
                .as_diagnostic(),
                pid: state.pid,
                base_url: base_url_for_port(state.port),
                health_url: state.health_url,
                ready_url: state.ready_url,
                binary_version: state.binary_version,
                binary_path: state.binary_path.map(|path| path.display().to_string()),
                log_path: state.log_path.map(|path| path.display().to_string()),
            },
            DaemonStatusSnapshot::Stale(state) => LocalGatewayDaemonStatus::Stale {
                identity: process_state::process_matches_record(
                    state.pid,
                    state.process_start_key.as_deref(),
                )
                .as_diagnostic(),
                pid: state.pid,
                binary_version: state.binary_version,
                log_path: state.log_path.map(|path| path.display().to_string()),
            },
            DaemonStatusSnapshot::Stopped => LocalGatewayDaemonStatus::Stopped,
        })
    }

    pub fn from_running_daemon() -> Result<Self, GatewayDiagnostic> {
        let status = Self::daemon_status().map_err(|error| {
            diagnostic(
                "daemon_state_unavailable",
                "The daemon state could not be read.",
                "Check the daemon state file and retry /remote.",
            )
            .with_context(redact_text(&error.to_string()))
        })?;

        let LocalGatewayDaemonStatus::Running { base_url, .. } = status else {
            return Err(match status {
                LocalGatewayDaemonStatus::Stale { pid, .. } => diagnostic(
                    "daemon_stale",
                    "The daemon state is stale.",
                    "Restart the daemon with `allthecodes daemon restart`.",
                )
                .with_context(format!("pid={pid}")),
                LocalGatewayDaemonStatus::Stopped => diagnostic(
                    "daemon_stopped",
                    "The daemon is not running.",
                    "Start it with `FEATURE_KAIROS=1 allthecodes daemon start`.",
                ),
                LocalGatewayDaemonStatus::Running { .. } => unreachable!(),
            });
        };

        let token = process_state::read_control_token()
            .map_err(|error| {
                diagnostic(
                    "control_token_unavailable",
                    "The daemon control token could not be read.",
                    "Check daemon token file permissions.",
                )
                .with_context(redact_text(&error.to_string()))
            })?
            .map(|token| token.token)
            .ok_or_else(|| {
                diagnostic(
                    "control_token_missing",
                    "The daemon control token is missing.",
                    "Restart the daemon so it can publish a fresh local control token.",
                )
            })?;

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|error| {
                diagnostic(
                    "gateway_client_unavailable",
                    "The local gateway HTTP client could not be created.",
                    "Retry the command or inspect the local TLS/network configuration.",
                )
                .with_context(redact_text(&error.to_string()))
            })?;

        Ok(Self {
            base_url,
            token,
            http,
        })
    }

    pub async fn capabilities(&self) -> Result<GatewayCapabilitiesSnapshot, GatewayDiagnostic> {
        self.get_json("/remote-control/v1/capabilities").await
    }

    pub async fn adapters(&self) -> Result<Vec<AdapterStatus>, GatewayDiagnostic> {
        Ok(self
            .get_json::<GatewayAdapters>("/remote-control/v1/adapters")
            .await?
            .adapters)
    }

    pub async fn channel_config(&self) -> Result<GatewayChannelsConfigResponse, GatewayDiagnostic> {
        self.get_json("/remote-control/v1/adapters/config").await
    }

    pub async fn update_adapter_config(
        &self,
        provider: AdapterProvider,
        patch: &Value,
    ) -> Result<GatewayChannelConfigSnapshot, GatewayDiagnostic> {
        self.patch_json(
            &format!("/remote-control/v1/adapters/{}/config", provider.as_str()),
            patch,
        )
        .await
    }

    pub async fn enable_adapter_config(
        &self,
        provider: AdapterProvider,
    ) -> Result<GatewayChannelConfigSnapshot, GatewayDiagnostic> {
        self.post_json(
            &format!("/remote-control/v1/adapters/{}/enable", provider.as_str()),
            &json!({}),
        )
        .await
    }

    pub async fn disable_adapter_config(
        &self,
        provider: AdapterProvider,
    ) -> Result<GatewayChannelConfigSnapshot, GatewayDiagnostic> {
        self.post_json(
            &format!("/remote-control/v1/adapters/{}/disable", provider.as_str()),
            &json!({}),
        )
        .await
    }

    pub async fn connect_adapter(
        &self,
        provider: AdapterProvider,
    ) -> Result<AdapterStatus, GatewayDiagnostic> {
        self.post_json(
            &format!("/remote-control/v1/adapters/{}/connect", provider.as_str()),
            &json!({}),
        )
        .await
    }

    pub async fn test_adapter_message(
        &self,
        provider: AdapterProvider,
        target: String,
        text: String,
    ) -> Result<AdapterStatus, GatewayDiagnostic> {
        self.post_json(
            &format!(
                "/remote-control/v1/adapters/{}/test-message",
                provider.as_str()
            ),
            &AdapterTestMessage { target, text },
        )
        .await
    }

    pub async fn show_run(&self, run_id: &RunId) -> Result<RunMeta, GatewayDiagnostic> {
        self.get_json(&format!("/remote-control/v1/runs/{run_id}"))
            .await
    }

    pub async fn run_output(&self, run_id: &RunId) -> Result<OutputReadBatch, GatewayDiagnostic> {
        self.get_json(&format!("/remote-control/v1/runs/{run_id}/events"))
            .await
    }

    pub async fn run_timeline(&self, run_id: &RunId) -> Result<Vec<RunEvent>, GatewayDiagnostic> {
        Ok(self
            .get_json::<GatewayRunTimeline>(&format!("/remote-control/v1/runs/{run_id}/timeline"))
            .await?
            .events)
    }

    pub async fn stop_run(
        &self,
        run_id: &RunId,
    ) -> Result<GatewayRunActionResponse, GatewayDiagnostic> {
        self.post_json(
            &format!("/remote-control/v1/runs/{run_id}/stop"),
            &json!({}),
        )
        .await
    }

    async fn get_json<T>(&self, path: &str) -> Result<T, GatewayDiagnostic>
    where
        T: DeserializeOwned,
    {
        let response = self
            .http
            .get(self.url(path))
            .header(TOKEN_HEADER, &self.token)
            .send()
            .await
            .map_err(http_transport_diagnostic)?;
        decode_response(response).await
    }

    async fn post_json<B, T>(&self, path: &str, body: &B) -> Result<T, GatewayDiagnostic>
    where
        B: serde::Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let response = self
            .http
            .post(self.url(path))
            .header(TOKEN_HEADER, &self.token)
            .json(body)
            .send()
            .await
            .map_err(http_transport_diagnostic)?;
        decode_response(response).await
    }

    async fn patch_json<B, T>(&self, path: &str, body: &B) -> Result<T, GatewayDiagnostic>
    where
        B: serde::Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let response = self
            .http
            .patch(self.url(path))
            .header(TOKEN_HEADER, &self.token)
            .json(body)
            .send()
            .await
            .map_err(http_transport_diagnostic)?;
        decode_response(response).await
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

async fn decode_response<T>(response: reqwest::Response) -> Result<T, GatewayDiagnostic>
where
    T: DeserializeOwned,
{
    let status = response.status();
    let text = response.text().await.map_err(http_transport_diagnostic)?;
    if status.is_success() {
        return serde_json::from_str(&text).map_err(|error| {
            diagnostic(
                "gateway_response_decode_failed",
                "The gateway response could not be decoded.",
                "Retry the command or inspect the daemon gateway route output.",
            )
            .with_context(redact_text(&error.to_string()))
        });
    }

    if let Ok(envelope) = serde_json::from_str::<GatewayErrorEnvelope>(&text) {
        return Err(envelope.error);
    }

    Err(diagnostic(
        "gateway_http_error",
        "The gateway returned an unexpected HTTP error.",
        "Retry the command or inspect daemon logs.",
    )
    .with_context(format!(
        "status={}, body={}",
        status_code(status),
        redact_text(&text)
    )))
}

#[derive(Debug, Deserialize)]
struct GatewayErrorEnvelope {
    error: GatewayDiagnostic,
}

fn http_transport_diagnostic(error: reqwest::Error) -> GatewayDiagnostic {
    diagnostic(
        "gateway_unreachable",
        "The local gateway HTTP API could not be reached.",
        "Verify the daemon is running and listening on loopback.",
    )
    .with_context(redact_text(&error.to_string()))
}

fn diagnostic(code: &str, message: &str, action: &str) -> GatewayDiagnostic {
    GatewayDiagnostic::new(code, message, action)
}

fn base_url_for_port(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn status_code(status: StatusCode) -> u16 {
    status.as_u16()
}

pub fn redact_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut redact_next = false;
    for part in value.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }

        if redact_next {
            out.push_str("<redacted>");
            redact_next = false;
            continue;
        }

        let lower = part.to_ascii_lowercase();
        if lower == "bearer" {
            out.push_str(part);
            redact_next = true;
            continue;
        }

        if let Some(redacted) = redact_key_value(part) {
            out.push_str(&redacted);
        } else {
            out.push_str(part);
        }
    }
    out
}

fn redact_key_value(part: &str) -> Option<String> {
    let split_at = part.find('=').or_else(|| part.find(':'))?;
    let (key, rest) = part.split_at(split_at);
    if !is_sensitive_key(key) {
        return None;
    }

    let delimiter = rest.chars().next().unwrap_or('=');
    Some(format!("{key}{delimiter}<redacted>"))
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-')
        .to_ascii_lowercase()
        .replace('-', "_");
    normalized == "authorization"
        || normalized == "apikey"
        || normalized == "password"
        || normalized.contains("token")
        || normalized.contains("secret")
        || normalized.contains("credential")
        || normalized.contains("signature")
        || normalized.ends_with("_key")
}

#[cfg(test)]
mod tests {
    use super::redact_text;

    #[test]
    fn redacts_credentials_without_hiding_diagnostic_codes() {
        let text = "code=control_token_missing Authorization: Bearer abc123 api_key=sk-test client-secret:top refresh_token=raw";
        let redacted = redact_text(text);

        assert!(redacted.contains("code=control_token_missing"));
        assert!(redacted.contains("Authorization:<redacted>"));
        assert!(redacted.contains("Bearer <redacted>"));
        assert!(redacted.contains("api_key=<redacted>"));
        assert!(redacted.contains("client-secret:<redacted>"));
        assert!(redacted.contains("refresh_token=<redacted>"));
        for secret in ["abc123", "sk-test", "top", "raw"] {
            assert!(!redacted.contains(secret));
        }
    }
}
