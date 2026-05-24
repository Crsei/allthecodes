use crate::{
    AdapterProvider, GatewayDiagnostic, GatewayError, GatewayStore, RunEvent, RunEventKind, RunMeta,
};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

const DELIVERY_FILE: &str = "delivery.ndjson";
const DEFAULT_CALLBACK_RETRIES: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum DeliveryTarget {
    Origin,
    Local,
    Callback {
        url: String,
    },
    Channel {
        provider: AdapterProvider,
        target: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thread: Option<String>,
    },
    SseSubscriber {
        client_id: String,
    },
}

impl DeliveryTarget {
    pub fn parse(value: &str) -> Result<Self, GatewayError> {
        if value == "origin" {
            return Ok(Self::Origin);
        }
        if value == "local" {
            return Ok(Self::Local);
        }
        if let Some(url) = value.strip_prefix("callback:") {
            if url.trim().is_empty() {
                return Err(delivery_error(
                    "callback_target_invalid",
                    "Callback delivery target is missing a URL.",
                    "Use callback:https://host/path or callback:http://127.0.0.1/path.",
                    "target=callback",
                ));
            }
            validate_callback_url(url)?;
            return Ok(Self::Callback {
                url: url.to_string(),
            });
        }
        if let Some(client_id) = value.strip_prefix("sse:") {
            if client_id.trim().is_empty() {
                return Err(delivery_error(
                    "sse_target_invalid",
                    "SSE delivery target is missing a client id.",
                    "Use sse:<client-id> for explicit subscriber delivery.",
                    "target=sse",
                ));
            }
            return Ok(Self::SseSubscriber {
                client_id: client_id.to_string(),
            });
        }
        if let Some(rest) = value.strip_prefix("channel:") {
            let mut parts = rest.splitn(3, ':');
            let provider = parts.next().unwrap_or_default();
            let target = parts.next().unwrap_or_default();
            let thread = parts.next().filter(|part| !part.trim().is_empty());
            if provider.is_empty() || target.is_empty() {
                return Err(delivery_error(
                    "channel_target_invalid",
                    "Channel delivery target is missing provider or target.",
                    "Use channel:<provider>:<target>[:thread].",
                    "target=channel",
                ));
            }
            return Ok(Self::Channel {
                provider: AdapterProvider::parse(provider)?,
                target: target.to_string(),
                thread: thread.map(ToOwned::to_owned),
            });
        }

        Err(delivery_error(
            "delivery_unsupported",
            "The requested delivery target is not supported.",
            "Use origin, local, callback:<url>, channel:<provider>:<target>, or sse:<client-id>.",
            format!("target={}", redact_target(value)),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryPayload {
    pub run_id: String,
    pub session_key: String,
    pub status: String,
    pub event: RunEvent,
}

impl DeliveryPayload {
    pub fn from_event(meta: &RunMeta, event: RunEvent) -> Self {
        Self {
            run_id: meta.run_id.to_string(),
            session_key: meta.session_key.to_string(),
            status: format!("{:?}", meta.status).to_ascii_lowercase(),
            event,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Delivered,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryRecord {
    pub target: DeliveryTarget,
    pub status: DeliveryStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<GatewayDiagnostic>,
}

pub trait CallbackDeliverySink {
    fn deliver_callback(&self, url: &str, payload: &DeliveryPayload) -> Result<(), GatewayError>;
}

pub trait ChannelDeliverySink {
    fn deliver_channel(
        &self,
        provider: AdapterProvider,
        target: &str,
        thread: Option<&str>,
        payload: &DeliveryPayload,
    ) -> Result<(), GatewayError>;
}

#[derive(Debug, Clone)]
pub struct DeliveryRouter {
    callback_retries: usize,
}

impl Default for DeliveryRouter {
    fn default() -> Self {
        Self {
            callback_retries: DEFAULT_CALLBACK_RETRIES,
        }
    }
}

impl DeliveryRouter {
    pub fn new(callback_retries: usize) -> Self {
        Self { callback_retries }
    }

    pub fn parse_targets(values: &[String]) -> Result<Vec<DeliveryTarget>, GatewayError> {
        values
            .iter()
            .map(|value| DeliveryTarget::parse(value))
            .collect()
    }

    pub fn deliver_event(
        &self,
        store: &GatewayStore,
        meta: &RunMeta,
        event: &RunEvent,
        callback_sink: &impl CallbackDeliverySink,
        channel_sink: &impl ChannelDeliverySink,
    ) -> Result<Vec<DeliveryRecord>, GatewayError> {
        let targets = Self::parse_targets(&meta.request.policy.delivery)?;
        let payload = DeliveryPayload::from_event(meta, event.clone());
        let mut records = Vec::with_capacity(targets.len());

        for target in targets {
            let record = self.deliver_target(&target, &payload, callback_sink, channel_sink);
            if matches!(target, DeliveryTarget::Local) || record.diagnostic.is_some() {
                append_delivery_record(store, meta, &record)?;
            }
            records.push(record);
        }

        Ok(records)
    }

    fn deliver_target(
        &self,
        target: &DeliveryTarget,
        payload: &DeliveryPayload,
        callback_sink: &impl CallbackDeliverySink,
        channel_sink: &impl ChannelDeliverySink,
    ) -> DeliveryRecord {
        match target {
            DeliveryTarget::Origin | DeliveryTarget::SseSubscriber { .. } => {
                delivery_record(target, DeliveryStatus::Skipped, None)
            }
            DeliveryTarget::Local => delivery_record(target, DeliveryStatus::Delivered, None),
            DeliveryTarget::Callback { url } => {
                match self.deliver_callback(url, payload, callback_sink) {
                    Ok(()) => delivery_record(target, DeliveryStatus::Delivered, None),
                    Err(error) => delivery_record(target, DeliveryStatus::Failed, Some(error)),
                }
            }
            DeliveryTarget::Channel {
                provider,
                target: channel_target,
                thread,
            } => match channel_sink.deliver_channel(
                *provider,
                channel_target,
                thread.as_deref(),
                payload,
            ) {
                Ok(()) => delivery_record(target, DeliveryStatus::Delivered, None),
                Err(error) => delivery_record(target, DeliveryStatus::Failed, Some(error)),
            },
        }
    }

    fn deliver_callback(
        &self,
        url: &str,
        payload: &DeliveryPayload,
        sink: &impl CallbackDeliverySink,
    ) -> Result<(), GatewayError> {
        validate_callback_url(url)?;
        let attempts = self.callback_retries.saturating_add(1);
        let mut last_error = None;
        for _ in 0..attempts {
            match sink.deliver_callback(url, payload) {
                Ok(()) => return Ok(()),
                Err(error) => last_error = Some(error),
            }
        }

        Err(last_error.unwrap_or_else(|| {
            delivery_error(
                "retry_exhausted",
                "Callback delivery retry budget was exhausted.",
                "Inspect the callback endpoint and retry delivery manually if needed.",
                format!("url={}", redact_target(url)),
            )
        }))
    }
}

fn append_delivery_record(
    store: &GatewayStore,
    meta: &RunMeta,
    record: &DeliveryRecord,
) -> Result<(), GatewayError> {
    let path = store.run_dir(&meta.run_id).join(DELIVERY_FILE);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| {
            GatewayError::io(
                "delivery_log_write_failed",
                "The gateway could not append to the run delivery log.",
                "Check permissions and filesystem state for delivery.ndjson.",
                &path,
                &error,
            )
        })?;

    serde_json::to_writer(&mut file, record).map_err(|error| {
        GatewayError::new(
            GatewayDiagnostic::new(
                "delivery_log_encode_failed",
                "The gateway could not encode the delivery record.",
                "Check the delivery target and diagnostic payload.",
            )
            .with_context(format!("run_id={}, error={}", meta.run_id, error)),
        )
    })?;
    file.write_all(b"\n").map_err(|error| {
        GatewayError::io(
            "delivery_log_write_failed",
            "The gateway could not finish writing to the run delivery log.",
            "Check permissions and available disk space for delivery.ndjson.",
            &path,
            &error,
        )
    })
}

fn delivery_record(
    target: &DeliveryTarget,
    status: DeliveryStatus,
    error: Option<GatewayError>,
) -> DeliveryRecord {
    DeliveryRecord {
        target: target.clone(),
        status,
        diagnostic: error.map(GatewayError::into_diagnostic),
    }
}

pub(crate) fn delivery_failed_event(
    run_id: crate::RunId,
    sequence: u64,
    diagnostic: GatewayDiagnostic,
) -> RunEvent {
    RunEvent::new(
        run_id,
        sequence,
        RunEventKind::DeliveryFailed { diagnostic },
    )
}

fn validate_callback_url(url: &str) -> Result<(), GatewayError> {
    let parsed = ParsedCallbackUrl::parse(url)?;
    if parsed.scheme == "https" {
        if parsed.host_is_forbidden() {
            return Err(callback_blocked(url, "private_or_metadata_host"));
        }
        return Ok(());
    }
    if parsed.scheme == "http" && parsed.host_is_loopback() {
        return Ok(());
    }
    if parsed.scheme == "http" {
        return Err(callback_blocked(url, "non_loopback_http"));
    }
    Err(delivery_error(
        "callback_blocked",
        "Callback delivery only supports HTTPS or loopback HTTP targets.",
        "Use an HTTPS callback URL, or http://127.0.0.1 for local testing.",
        format!("scheme={}", parsed.scheme),
    ))
}

fn callback_blocked(url: &str, reason: &str) -> GatewayError {
    delivery_error(
        "callback_blocked",
        "Callback delivery target was rejected by SSRF protection.",
        "Use HTTPS with a public callback host, or loopback HTTP for local testing.",
        format!("reason={}, url={}", reason, redact_target(url)),
    )
}

fn delivery_error(
    code: impl Into<String>,
    message: impl Into<String>,
    action: impl Into<String>,
    context: impl Into<String>,
) -> GatewayError {
    GatewayError::new(GatewayDiagnostic::new(code, message, action).with_context(context))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedCallbackUrl {
    scheme: String,
    host: String,
}

impl ParsedCallbackUrl {
    fn parse(url: &str) -> Result<Self, GatewayError> {
        let (scheme, rest) = url.split_once("://").ok_or_else(|| {
            delivery_error(
                "callback_target_invalid",
                "Callback delivery target is not a valid absolute URL.",
                "Use callback:https://host/path or callback:http://127.0.0.1/path.",
                format!("url={}", redact_target(url)),
            )
        })?;
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let host = authority
            .rsplit_once('@')
            .map(|(_, host)| host)
            .unwrap_or(authority);
        let host = host
            .split_once(':')
            .map(|(host, _)| host)
            .unwrap_or(host)
            .trim_matches(['[', ']']);
        if scheme.is_empty() || host.is_empty() {
            return Err(delivery_error(
                "callback_target_invalid",
                "Callback delivery target is missing a scheme or host.",
                "Use callback:https://host/path or callback:http://127.0.0.1/path.",
                format!("url={}", redact_target(url)),
            ));
        }
        Ok(Self {
            scheme: scheme.to_ascii_lowercase(),
            host: host.to_ascii_lowercase(),
        })
    }

    fn host_is_loopback(&self) -> bool {
        matches!(self.host.as_str(), "localhost")
            || self
                .host
                .parse::<IpAddr>()
                .map(|ip| ip.is_loopback())
                .unwrap_or(false)
    }

    fn host_is_forbidden(&self) -> bool {
        if matches!(
            self.host.as_str(),
            "localhost" | "metadata.google.internal" | "metadata"
        ) {
            return true;
        }
        match self.host.parse::<IpAddr>() {
            Ok(IpAddr::V4(ip)) => forbidden_ipv4(ip),
            Ok(IpAddr::V6(ip)) => forbidden_ipv6(ip),
            Err(_) => false,
        }
    }
}

fn forbidden_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.octets()[0] == 0
        || ip == Ipv4Addr::new(169, 254, 169, 254)
}

fn forbidden_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback() || ip.is_unspecified()
}

fn redact_target(value: &str) -> String {
    let value = value.replace(['\r', '\n'], "");
    if value.len() <= 80 {
        value
    } else {
        format!("{}...", &value[..80])
    }
}
