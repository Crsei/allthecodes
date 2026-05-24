use super::{
    adapter_diagnostic, AdapterProvider, AdapterStatus, AdapterTestMessage, RemoteAdapter,
};
use crate::config::TelegramAdapterConfig;
use crate::GatewayError;
use serde_json::{json, Value};
use std::sync::Arc;

const TELEGRAM_PROVIDER: AdapterProvider = AdapterProvider::Telegram;

#[derive(Clone)]
pub struct TelegramAdapter {
    config: TelegramAdapterConfig,
    transport: Arc<dyn TelegramTransport>,
}

impl TelegramAdapter {
    pub fn new(config: TelegramAdapterConfig) -> Self {
        Self::with_transport(
            config.clone(),
            HttpTelegramTransport::new(config.api_base_url.clone()),
        )
    }

    pub fn with_transport<T>(config: TelegramAdapterConfig, transport: T) -> Self
    where
        T: TelegramTransport + 'static,
    {
        Self {
            config,
            transport: Arc::new(transport),
        }
    }

    fn token(&self) -> Result<&str, Box<AdapterStatus>> {
        self.config
            .bot_token
            .as_deref()
            .filter(|token| !token.trim().is_empty())
            .ok_or_else(|| {
                Box::new(AdapterStatus::unconfigured(
                    TELEGRAM_PROVIDER,
                    "Telegram bot token is not configured.",
                ))
            })
    }

    fn target_allowed(&self, target: &str) -> bool {
        self.config.test_chat_allowlist.is_empty()
            || self
                .config
                .test_chat_allowlist
                .iter()
                .any(|allowed| allowed == target)
    }
}

impl RemoteAdapter for TelegramAdapter {
    fn provider(&self) -> AdapterProvider {
        TELEGRAM_PROVIDER
    }

    fn status(&self) -> AdapterStatus {
        if self.token().is_err() {
            return AdapterStatus::unconfigured(
                TELEGRAM_PROVIDER,
                "Telegram bot token is not configured.",
            );
        }

        AdapterStatus::blocked(
            TELEGRAM_PROVIDER,
            "telegram_not_connected",
            "Telegram credentials are configured but have not been checked.",
            "Run adapter connect to verify the Telegram bot token.",
        )
    }

    fn connect(&self) -> Result<AdapterStatus, GatewayError> {
        let token = self.token().map_err(|status| status_error(*status))?;
        let response = self
            .transport
            .call(token, "getMe", Value::Object(Default::default()))?;
        ensure_ok("getMe", response)?;

        if self.config.probe_updates {
            let response =
                self.transport
                    .call(token, "getUpdates", json!({ "limit": 1, "timeout": 0 }))?;
            ensure_ok("getUpdates", response)?;
        }

        Ok(AdapterStatus::connected_outbound_only(
            TELEGRAM_PROVIDER,
            "Telegram bot token is valid; inbound conversation control is not enabled.",
        ))
    }

    fn test_message(&self, message: AdapterTestMessage) -> Result<AdapterStatus, GatewayError> {
        let token = self.token().map_err(|status| status_error(*status))?;
        if message.target.trim().is_empty() {
            return Err(GatewayError::new(adapter_diagnostic(
                TELEGRAM_PROVIDER,
                "telegram_target_missing",
                "Telegram test-message target is missing.",
                "Provide a configured Telegram chat id.",
            )));
        }
        if !self.target_allowed(&message.target) {
            return Err(GatewayError::new(adapter_diagnostic(
                TELEGRAM_PROVIDER,
                "telegram_target_blocked",
                "Telegram test-message target is not allowlisted.",
                "Add the chat id to the Telegram adapter test allowlist.",
            )));
        }

        let response = self.transport.call(
            token,
            "sendMessage",
            json!({
                "chat_id": message.target,
                "text": message.text,
                "disable_web_page_preview": true
            }),
        )?;
        ensure_ok("sendMessage", response)?;

        Ok(AdapterStatus::connected_outbound_only(
            TELEGRAM_PROVIDER,
            "Telegram test message was accepted for delivery.",
        ))
    }
}

pub trait TelegramTransport: Send + Sync {
    fn call(&self, token: &str, method: &str, body: Value) -> Result<Value, GatewayError>;
}

struct HttpTelegramTransport {
    api_base_url: String,
    client: reqwest::blocking::Client,
}

impl HttpTelegramTransport {
    fn new(api_base_url: String) -> Self {
        Self {
            api_base_url: api_base_url.trim_end_matches('/').to_string(),
            client: http_client_for_base_url(&api_base_url),
        }
    }
}

impl TelegramTransport for HttpTelegramTransport {
    fn call(&self, token: &str, method: &str, body: Value) -> Result<Value, GatewayError> {
        let url = format!("{}/bot{}/{}", self.api_base_url, token, method);
        let response = self
            .client
            .post(url)
            .json(&body)
            .send()
            .map_err(|_| telegram_http_error(method, None))?;
        let status = response.status();
        let value = response
            .json::<Value>()
            .map_err(|_| telegram_http_error(method, Some(status.as_u16())))?;
        if !status.is_success() {
            return Err(telegram_http_error(method, Some(status.as_u16())));
        }
        Ok(value)
    }
}

fn telegram_http_error(method: &str, status: Option<u16>) -> GatewayError {
    let status_context = status
        .map(|status| format!(", http_status={status}"))
        .unwrap_or_default();
    GatewayError::new(
        telegram_diagnostic(
            "telegram_http_failed",
            "Telegram adapter HTTP request failed.",
            "Verify network access, Telegram API availability, and adapter credentials.",
            method,
        )
        .with_context(format!(
            "provider=telegram, method={method}{status_context}"
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

fn ensure_ok(method: &'static str, response: Value) -> Result<(), GatewayError> {
    if response.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return Ok(());
    }

    let error_code = response
        .get("error_code")
        .and_then(Value::as_i64)
        .map(|code| format!(", telegram_error_code={code}"))
        .unwrap_or_default();

    Err(GatewayError::new(
        telegram_diagnostic(
            "telegram_api_rejected",
            "Telegram rejected the adapter request.",
            "Verify the Telegram adapter token, bot permissions, and target configuration.",
            method,
        )
        .with_context(format!("provider=telegram, method={method}{error_code}")),
    ))
}

fn status_error(status: AdapterStatus) -> GatewayError {
    GatewayError::new(status.diagnostic.unwrap_or_else(|| {
        adapter_diagnostic(
            TELEGRAM_PROVIDER,
            "telegram_adapter_blocked",
            status.message,
            "Check Telegram adapter configuration.",
        )
    }))
}

fn telegram_diagnostic(
    code: impl Into<String>,
    message: impl Into<String>,
    action: impl Into<String>,
    method: impl AsRef<str>,
) -> crate::GatewayDiagnostic {
    crate::GatewayDiagnostic::new(code, message, action)
        .with_context(format!("provider=telegram, method={}", method.as_ref()))
}
