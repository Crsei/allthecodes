use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::webhook_hmac::{bad_hmac_error, verify_hex_hmac};
use crate::webhook_render::{render_prompt, source_for};
use crate::{BusyPolicy, GatewayDiagnostic, GatewayError, RemoteSource, RunPolicy, RunRequest};

pub const GITHUB_SIGNATURE_HEADER: &str = "x-hub-signature-256";
pub const SLACK_SIGNATURE_HEADER: &str = "x-slack-signature";
pub const SLACK_TIMESTAMP_HEADER: &str = "x-slack-request-timestamp";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookProvider {
    GitHub,
    Slack,
    Generic,
}

impl WebhookProvider {
    fn default_signature_header(&self) -> Option<&'static str> {
        match self {
            Self::GitHub => Some(GITHUB_SIGNATURE_HEADER),
            Self::Slack => Some(SLACK_SIGNATURE_HEADER),
            Self::Generic => None,
        }
    }

    fn event_header(&self) -> Option<&'static str> {
        match self {
            Self::GitHub => Some("x-github-event"),
            Self::Slack => Some("x-slack-event-type"),
            Self::Generic => None,
        }
    }

    fn idempotency_header(&self) -> Option<&'static str> {
        match self {
            Self::GitHub => Some("x-github-delivery"),
            Self::Slack => Some("x-slack-request-timestamp"),
            Self::Generic => Some("idempotency-key"),
        }
    }

    pub fn as_source_client(&self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::Slack => "slack",
            Self::Generic => "generic",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookSourceTemplate {
    #[serde(default = "default_tenant")]
    pub tenant: String,
    #[serde(default = "default_workspace")]
    pub workspace: String,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
}

impl Default for WebhookSourceTemplate {
    fn default() -> Self {
        Self {
            tenant: default_tenant(),
            workspace: default_workspace(),
            client_id: None,
            user_id: None,
            thread_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookRouteConfig {
    pub route_id: String,
    pub provider: WebhookProvider,
    #[serde(default)]
    pub accepted_events: Vec<String>,
    #[serde(default, skip_serializing)]
    pub secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_header: Option<String>,
    #[serde(default = "default_prompt_template")]
    pub prompt_template: String,
    #[serde(default)]
    pub default_source: WebhookSourceTemplate,
    #[serde(default)]
    pub busy: BusyPolicy,
    #[serde(default = "default_delivery")]
    pub delivery: Vec<String>,
    #[serde(default)]
    pub deliver_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_header: Option<String>,
    #[serde(default)]
    pub max_body_bytes: usize,
}

impl WebhookRouteConfig {
    pub fn new(route_id: impl Into<String>, secret: Option<String>) -> Self {
        Self {
            route_id: route_id.into(),
            provider: WebhookProvider::GitHub,
            accepted_events: Vec::new(),
            secret,
            secret_env: None,
            signature_header: None,
            prompt_template: default_prompt_template(),
            default_source: WebhookSourceTemplate::default(),
            busy: BusyPolicy::Queue,
            delivery: default_delivery(),
            deliver_only: false,
            idempotency_header: None,
            max_body_bytes: 256 * 1024,
        }
    }

    pub fn github(route_id: impl Into<String>, secret: Option<String>) -> Self {
        Self {
            provider: WebhookProvider::GitHub,
            accepted_events: vec![
                "pull_request".to_string(),
                "pull_request_review".to_string(),
                "issue_comment".to_string(),
            ],
            ..Self::new(route_id, secret)
        }
    }

    pub fn slack(route_id: impl Into<String>, secret: Option<String>) -> Self {
        Self {
            provider: WebhookProvider::Slack,
            accepted_events: Vec::new(),
            ..Self::new(route_id, secret)
        }
    }

    pub fn generic(route_id: impl Into<String>, secret: Option<String>) -> Self {
        Self {
            provider: WebhookProvider::Generic,
            accepted_events: Vec::new(),
            ..Self::new(route_id, secret)
        }
    }

    pub fn with_deliver_only(mut self, deliver_only: bool) -> Self {
        self.deliver_only = deliver_only;
        self
    }

    pub fn resolved_secret(&self) -> Option<String> {
        self.secret
            .clone()
            .or_else(|| {
                self.secret_env
                    .as_deref()
                    .and_then(|key| std::env::var(key).ok())
            })
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }
}

#[derive(Debug, Clone, Default)]
pub struct WebhookVerifier;

impl WebhookVerifier {
    pub fn verify_github(
        route: &WebhookRouteConfig,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Result<(), GatewayError> {
        verify_size(route, body)?;
        let secret = route_secret_owned(route)?;
        let signature = headers
            .get(GITHUB_SIGNATURE_HEADER)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(missing_signature_error)?;
        let Some(hex_signature) = signature.strip_prefix("sha256=") else {
            return Err(bad_hmac_error());
        };
        verify_hex_hmac(secret.as_bytes(), body, hex_signature)
    }

    pub fn verify(
        route: &WebhookRouteConfig,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Result<(), GatewayError> {
        verify_size(route, body)?;
        match route.provider {
            WebhookProvider::GitHub => verify_provider_hmac(route, headers, body, "sha256="),
            WebhookProvider::Slack => verify_slack(route, headers, body),
            WebhookProvider::Generic => {
                if route.resolved_secret().is_some() {
                    verify_provider_hmac(route, headers, body, "sha256=")
                } else {
                    Err(GatewayError::new(GatewayDiagnostic::new(
                        "webhook_disabled",
                        "Webhook route is disabled because its secret is missing.",
                        "Configure a per-route webhook secret before enabling this route.",
                    )))
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum WebhookRouteOutcome {
    Run {
        request: RunRequest,
        event: Option<String>,
    },
    DeliverOnly {
        event: Option<String>,
        prompt: String,
        source: RemoteSource,
        idempotency_key: Option<String>,
    },
    Ignored {
        event: Option<String>,
    },
}

pub struct WebhookRouter;

impl WebhookRouter {
    pub fn handle(
        route: &WebhookRouteConfig,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Result<WebhookRouteOutcome, GatewayError> {
        WebhookVerifier::verify(route, headers, body)?;
        let payload = parse_body(body)?;
        let event = event_name(route, headers, &payload);
        if !event_allowed(route, event.as_deref()) {
            return Ok(WebhookRouteOutcome::Ignored { event });
        }

        let idempotency_key = idempotency_key(route, headers, &payload);
        let source = source_for(route, headers, &payload, idempotency_key.as_deref());
        let prompt = render_prompt(route, event.as_deref(), &payload);
        if route.deliver_only {
            Ok(WebhookRouteOutcome::DeliverOnly {
                event,
                prompt,
                source,
                idempotency_key,
            })
        } else {
            Ok(WebhookRouteOutcome::Run {
                event,
                request: RunRequest {
                    prompt,
                    source,
                    policy: RunPolicy {
                        busy: route.busy,
                        permission_mode: "ask".to_string(),
                        delivery: route.delivery.clone(),
                    },
                    idempotency_key,
                },
            })
        }
    }
}

fn verify_size(route: &WebhookRouteConfig, body: &[u8]) -> Result<(), GatewayError> {
    if body.len() <= route.max_body_bytes {
        Ok(())
    } else {
        Err(GatewayError::new(GatewayDiagnostic::new(
            "payload_too_large",
            "The remote-control gateway payload is too large.",
            "Reduce the webhook body size and retry.",
        )))
    }
}

fn route_secret_owned(route: &WebhookRouteConfig) -> Result<String, GatewayError> {
    route.resolved_secret().ok_or_else(|| {
        GatewayError::new(GatewayDiagnostic::new(
            "webhook_disabled",
            "Webhook route is disabled because its secret is missing.",
            "Configure a per-route webhook secret before enabling this route.",
        ))
    })
}

fn verify_provider_hmac(
    route: &WebhookRouteConfig,
    headers: &HeaderMap,
    body: &[u8],
    prefix: &str,
) -> Result<(), GatewayError> {
    let secret = route_secret_owned(route)?;
    let header_name = route
        .signature_header
        .as_deref()
        .or_else(|| route.provider.default_signature_header())
        .unwrap_or(GITHUB_SIGNATURE_HEADER);
    let signature = header_str(headers, header_name).ok_or_else(missing_signature_error)?;
    let Some(hex_signature) = signature.strip_prefix(prefix) else {
        return Err(bad_hmac_error());
    };
    verify_hex_hmac(secret.as_bytes(), body, hex_signature)
}

fn verify_slack(
    route: &WebhookRouteConfig,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(), GatewayError> {
    let secret = route_secret_owned(route)?;
    let signature =
        header_str(headers, SLACK_SIGNATURE_HEADER).ok_or_else(missing_signature_error)?;
    let timestamp =
        header_str(headers, SLACK_TIMESTAMP_HEADER).ok_or_else(missing_signature_error)?;
    let base = format!("v0:{}:{}", timestamp, String::from_utf8_lossy(body));
    let Some(hex_signature) = signature.strip_prefix("v0=") else {
        return Err(bad_hmac_error());
    };
    verify_hex_hmac(secret.as_bytes(), base.as_bytes(), hex_signature)
}

fn missing_signature_error() -> GatewayError {
    GatewayError::new(GatewayDiagnostic::new(
        "missing_signature",
        "Webhook request is missing its signature header.",
        "Send the provider HMAC signature header with the webhook request.",
    ))
}

fn parse_body(body: &[u8]) -> Result<Value, GatewayError> {
    serde_json::from_slice(body).map_err(|error| {
        GatewayError::new(
            GatewayDiagnostic::new(
                "invalid_json",
                "The webhook body is not valid JSON.",
                "Send a JSON webhook body that matches the route provider.",
            )
            .with_context(format!("error={}", error)),
        )
    })
}

fn event_name(route: &WebhookRouteConfig, headers: &HeaderMap, payload: &Value) -> Option<String> {
    route
        .provider
        .event_header()
        .and_then(|name| header_str(headers, name))
        .or_else(|| {
            payload
                .get("event")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            payload
                .get("type")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn event_allowed(route: &WebhookRouteConfig, event: Option<&str>) -> bool {
    route.accepted_events.is_empty()
        || event
            .map(|event| {
                route
                    .accepted_events
                    .iter()
                    .any(|accepted| accepted == "*" || accepted.eq_ignore_ascii_case(event))
            })
            .unwrap_or(false)
}

fn idempotency_key(
    route: &WebhookRouteConfig,
    headers: &HeaderMap,
    payload: &Value,
) -> Option<String> {
    route
        .idempotency_header
        .as_deref()
        .or_else(|| route.provider.idempotency_header())
        .and_then(|name| header_str(headers, name))
        .or_else(|| {
            payload
                .get("delivery_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            payload
                .get("id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn default_tenant() -> String {
    "local".to_string()
}

fn default_workspace() -> String {
    std::env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| ".".to_string())
}

fn default_prompt_template() -> String {
    "Webhook {{provider}}/{{route_id}} received {{event}} for {{repository}}: {{action}}\n{{body}}"
        .to_string()
}

fn default_delivery() -> Vec<String> {
    vec!["origin".to_string(), "local".to_string()]
}
