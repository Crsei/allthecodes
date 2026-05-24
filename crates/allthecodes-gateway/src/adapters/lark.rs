use super::{
    adapter_diagnostic, AdapterProvider, AdapterStatus, AdapterTestMessage, RemoteAdapter,
};
use crate::config::LarkAdapterConfig;
use crate::GatewayError;
use serde_json::Value;
use std::sync::Arc;

const LARK_PROVIDER: AdapterProvider = AdapterProvider::Lark;

#[derive(Clone)]
pub struct LarkAdapter {
    config: LarkAdapterConfig,
    transport: Arc<dyn LarkTransport>,
}

impl LarkAdapter {
    pub fn new(config: LarkAdapterConfig) -> Self {
        Self::with_transport(
            config.clone(),
            HttpLarkTransport::new(config.api_base_url.clone()),
        )
    }

    pub fn with_transport<T>(config: LarkAdapterConfig, transport: T) -> Self
    where
        T: LarkTransport + 'static,
    {
        Self {
            config,
            transport: Arc::new(transport),
        }
    }

    fn mode(&self) -> Result<LarkConnectionMode<'_>, Box<AdapterStatus>> {
        let webhook_url = self
            .config
            .outbound_webhook_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(webhook_url) = webhook_url {
            return Ok(LarkConnectionMode::Webhook { webhook_url });
        }

        let app_id = self
            .config
            .app_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let app_secret = self
            .config
            .app_secret
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let (Some(app_id), Some(app_secret)) = (app_id, app_secret) {
            return Ok(LarkConnectionMode::AppCredentials { app_id, app_secret });
        }

        Err(Box::new(AdapterStatus::blocked(
            LARK_PROVIDER,
            "lark_credentials_missing",
            "Lark adapter credentials are missing.",
            "Configure an outbound webhook URL or both Lark app_id and app_secret.",
        )))
    }

    fn target_allowed(&self, target: &str) -> bool {
        self.config.test_target_allowlist.is_empty()
            || self
                .config
                .test_target_allowlist
                .iter()
                .any(|allowed| allowed == target)
    }
}

impl RemoteAdapter for LarkAdapter {
    fn provider(&self) -> AdapterProvider {
        LARK_PROVIDER
    }

    fn status(&self) -> AdapterStatus {
        if !self.config.enabled {
            return AdapterStatus::unconfigured(LARK_PROVIDER, "Lark adapter is not enabled.");
        }

        match self.mode() {
            Ok(LarkConnectionMode::Webhook { .. }) => AdapterStatus::blocked(
                LARK_PROVIDER,
                "lark_not_connected",
                "Lark outbound webhook is configured but has not been checked.",
                "Run adapter connect to verify outbound-only Lark delivery.",
            ),
            Ok(LarkConnectionMode::AppCredentials { .. }) => AdapterStatus::blocked(
                LARK_PROVIDER,
                "lark_not_connected",
                "Lark app credentials are configured but have not been checked.",
                "Run adapter connect to verify outbound-only Lark delivery.",
            ),
            Err(status) => *status,
        }
    }

    fn connect(&self) -> Result<AdapterStatus, GatewayError> {
        match self.mode().map_err(|status| status_error(*status))? {
            LarkConnectionMode::Webhook { webhook_url } => {
                self.transport.probe_webhook(webhook_url)?;
            }
            LarkConnectionMode::AppCredentials { app_id, app_secret } => {
                let response = self.transport.tenant_access_token(app_id, app_secret)?;
                ensure_lark_ok("tenant_access_token", response)?;
            }
        }

        Ok(AdapterStatus::connected_outbound_only(
            LARK_PROVIDER,
            "Lark credentials are valid for outbound delivery; inbound event control is not enabled.",
        ))
    }

    fn test_message(&self, message: AdapterTestMessage) -> Result<AdapterStatus, GatewayError> {
        if message.target.trim().is_empty() {
            return Err(GatewayError::new(adapter_diagnostic(
                LARK_PROVIDER,
                "lark_target_missing",
                "Lark test-message target is missing.",
                "Provide a configured Lark target or webhook route name.",
            )));
        }
        if !self.target_allowed(&message.target) {
            return Err(GatewayError::new(adapter_diagnostic(
                LARK_PROVIDER,
                "lark_target_blocked",
                "Lark test-message target is not allowlisted.",
                "Add the target to the Lark adapter test allowlist.",
            )));
        }

        match self.mode().map_err(|status| status_error(*status))? {
            LarkConnectionMode::Webhook { webhook_url } => {
                self.transport.send_webhook(webhook_url, &message.text)?;
            }
            LarkConnectionMode::AppCredentials { app_id, app_secret } => {
                let token_response = self.transport.tenant_access_token(app_id, app_secret)?;
                let tenant_access_token = tenant_access_token(token_response)?;
                self.transport.send_app_message(
                    &tenant_access_token,
                    &message.target,
                    &message.text,
                )?;
            }
        }

        Ok(AdapterStatus::connected_outbound_only(
            LARK_PROVIDER,
            "Lark test message was accepted for outbound delivery.",
        ))
    }
}

pub trait LarkTransport: Send + Sync {
    fn tenant_access_token(&self, app_id: &str, app_secret: &str) -> Result<Value, GatewayError>;
    fn probe_webhook(&self, webhook_url: &str) -> Result<(), GatewayError>;
    fn send_webhook(&self, webhook_url: &str, text: &str) -> Result<(), GatewayError>;
    fn send_app_message(
        &self,
        tenant_access_token: &str,
        target: &str,
        text: &str,
    ) -> Result<(), GatewayError>;
}

struct HttpLarkTransport {
    api_base_url: String,
    client: reqwest::blocking::Client,
}

impl HttpLarkTransport {
    fn new(api_base_url: String) -> Self {
        Self {
            api_base_url: api_base_url.trim_end_matches('/').to_string(),
            client: http_client_for_base_url(&api_base_url),
        }
    }
}

impl LarkTransport for HttpLarkTransport {
    fn tenant_access_token(&self, app_id: &str, app_secret: &str) -> Result<Value, GatewayError> {
        let url = format!(
            "{}/open-apis/auth/v3/tenant_access_token/internal",
            self.api_base_url
        );
        let response = self
            .client
            .post(url)
            .json(&serde_json::json!({
                "app_id": app_id,
                "app_secret": app_secret,
            }))
            .send()
            .map_err(|_| lark_http_error("tenant_access_token", None))?;
        response_json(response, "tenant_access_token")
    }

    fn probe_webhook(&self, webhook_url: &str) -> Result<(), GatewayError> {
        self.send_webhook(webhook_url, "allthecodes gateway Lark adapter health check")
    }

    fn send_webhook(&self, webhook_url: &str, text: &str) -> Result<(), GatewayError> {
        let response = self
            .client
            .post(webhook_url)
            .json(&serde_json::json!({
                "msg_type": "text",
                "content": { "text": text },
            }))
            .send()
            .map_err(|_| lark_http_error("send_webhook", None))?;
        let value = response_json(response, "send_webhook")?;
        ensure_lark_ok("send_webhook", value)
    }

    fn send_app_message(
        &self,
        tenant_access_token: &str,
        target: &str,
        text: &str,
    ) -> Result<(), GatewayError> {
        let url = format!(
            "{}/open-apis/im/v1/messages?receive_id_type=chat_id",
            self.api_base_url
        );
        let content = serde_json::json!({ "text": text }).to_string();
        let response = self
            .client
            .post(url)
            .bearer_auth(tenant_access_token)
            .json(&serde_json::json!({
                "receive_id": target,
                "msg_type": "text",
                "content": content,
            }))
            .send()
            .map_err(|_| lark_http_error("send_app_message", None))?;
        let value = response_json(response, "send_app_message")?;
        ensure_lark_ok("send_app_message", value)
    }
}

fn response_json(
    response: reqwest::blocking::Response,
    operation: &'static str,
) -> Result<Value, GatewayError> {
    let status = response.status();
    let value = response
        .json::<Value>()
        .map_err(|_| lark_http_error(operation, Some(status.as_u16())))?;
    if !status.is_success() {
        return Err(lark_http_error(operation, Some(status.as_u16())));
    }
    Ok(value)
}

fn lark_http_error(operation: &'static str, status: Option<u16>) -> GatewayError {
    let status_context = status
        .map(|status| format!(", http_status={status}"))
        .unwrap_or_default();
    GatewayError::new(
        lark_diagnostic(
            "lark_http_failed",
            "Lark adapter HTTP request failed.",
            "Verify network access, Lark API availability, and adapter credentials.",
            operation,
        )
        .with_context(format!(
            "provider=lark, operation={operation}{status_context}"
        )),
    )
}

fn http_client_for_base_url(api_base_url: &str) -> reqwest::blocking::Client {
    let builder = reqwest::blocking::Client::builder();
    let builder = if is_loopback_base_url(api_base_url) {
        builder.no_proxy()
    } else {
        builder
    };
    builder
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

fn is_loopback_base_url(api_base_url: &str) -> bool {
    reqwest::Url::parse(api_base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some_and(|host| matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1"))
}

fn ensure_lark_ok(operation: &'static str, response: Value) -> Result<(), GatewayError> {
    let code = response
        .get("code")
        .and_then(Value::as_i64)
        .or_else(|| response.get("StatusCode").and_then(Value::as_i64))
        .unwrap_or(0);
    if code == 0 {
        return Ok(());
    }

    Err(GatewayError::new(
        lark_diagnostic(
            "lark_api_rejected",
            "Lark rejected the adapter request.",
            "Verify the Lark adapter credentials, app permissions, and target configuration.",
            operation,
        )
        .with_context(format!(
            "provider=lark, operation={operation}, lark_code={code}"
        )),
    ))
}

fn tenant_access_token(response: Value) -> Result<String, GatewayError> {
    ensure_lark_ok("tenant_access_token", response.clone())?;
    response
        .get("tenant_access_token")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| {
            GatewayError::new(lark_diagnostic(
                "lark_token_missing",
                "Lark did not return a tenant access token.",
                "Verify the app credentials and Lark tenant permissions.",
                "tenant_access_token",
            ))
        })
}

fn status_error(status: AdapterStatus) -> GatewayError {
    GatewayError::new(status.diagnostic.unwrap_or_else(|| {
        adapter_diagnostic(
            LARK_PROVIDER,
            "lark_adapter_blocked",
            status.message,
            "Check Lark adapter configuration.",
        )
    }))
}

fn lark_diagnostic(
    code: impl Into<String>,
    message: impl Into<String>,
    action: impl Into<String>,
    operation: impl AsRef<str>,
) -> crate::GatewayDiagnostic {
    crate::GatewayDiagnostic::new(code, message, action)
        .with_context(format!("provider=lark, operation={}", operation.as_ref()))
}

enum LarkConnectionMode<'a> {
    Webhook {
        webhook_url: &'a str,
    },
    AppCredentials {
        app_id: &'a str,
        app_secret: &'a str,
    },
}
