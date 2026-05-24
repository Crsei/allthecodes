use crate::{GatewayDiagnostic, GatewayError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub mod lark;
pub mod telegram;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterProvider {
    Lark,
    Telegram,
}

impl AdapterProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lark => "lark",
            Self::Telegram => "telegram",
        }
    }

    pub fn parse(value: &str) -> Result<Self, GatewayError> {
        match value {
            "lark" => Ok(Self::Lark),
            "telegram" => Ok(Self::Telegram),
            _ => Err(GatewayError::new(GatewayDiagnostic::new(
                "adapter_unsupported",
                "The requested adapter provider is not supported.",
                "Use one of the providers returned by GET /remote-control/v1/adapters.",
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterState {
    Unconfigured,
    Connected,
    ConnectedOutboundOnly,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterStatus {
    pub provider: AdapterProvider,
    pub configured: bool,
    pub state: AdapterState,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<GatewayDiagnostic>,
}

impl AdapterStatus {
    pub fn unconfigured(provider: AdapterProvider, message: impl Into<String>) -> Self {
        Self {
            provider,
            configured: false,
            state: AdapterState::Unconfigured,
            message: message.into(),
            diagnostic: Some(adapter_diagnostic(
                provider,
                "adapter_blocked",
                "Adapter credentials are not configured.",
                "Configure the provider credentials before connecting.",
            )),
        }
    }

    pub fn connected_outbound_only(provider: AdapterProvider, message: impl Into<String>) -> Self {
        Self {
            provider,
            configured: true,
            state: AdapterState::ConnectedOutboundOnly,
            message: message.into(),
            diagnostic: None,
        }
    }

    pub fn blocked(
        provider: AdapterProvider,
        code: impl Into<String>,
        message: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        let message = message.into();
        Self {
            provider,
            configured: true,
            state: AdapterState::Blocked,
            message: message.clone(),
            diagnostic: Some(adapter_diagnostic(provider, code, message, action)),
        }
    }

    pub fn failed(
        provider: AdapterProvider,
        code: impl Into<String>,
        message: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        let message = message.into();
        Self {
            provider,
            configured: true,
            state: AdapterState::Failed,
            message: message.clone(),
            diagnostic: Some(adapter_diagnostic(provider, code, message, action)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterTestMessage {
    pub target: String,
    pub text: String,
}

pub trait RemoteAdapter: Send + Sync {
    fn provider(&self) -> AdapterProvider;
    fn status(&self) -> AdapterStatus;
    fn connect(&self) -> Result<AdapterStatus, GatewayError>;
    fn test_message(&self, message: AdapterTestMessage) -> Result<AdapterStatus, GatewayError>;
}

#[derive(Default)]
pub struct AdapterRegistry {
    adapters: BTreeMap<AdapterProvider, Box<dyn RemoteAdapter>>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<A>(&mut self, adapter: A)
    where
        A: RemoteAdapter + 'static,
    {
        self.adapters.insert(adapter.provider(), Box::new(adapter));
    }

    pub fn get(&self, provider: AdapterProvider) -> Option<&dyn RemoteAdapter> {
        self.adapters.get(&provider).map(|adapter| adapter.as_ref())
    }

    pub fn statuses(&self) -> Vec<AdapterStatus> {
        self.adapters
            .values()
            .map(|adapter| adapter.status())
            .collect()
    }

    pub fn connect(&self, provider: AdapterProvider) -> Result<AdapterStatus, GatewayError> {
        let adapter = self.require(provider)?;
        adapter.connect()
    }

    pub fn test_message(
        &self,
        provider: AdapterProvider,
        message: AdapterTestMessage,
    ) -> Result<AdapterStatus, GatewayError> {
        let adapter = self.require(provider)?;
        adapter.test_message(message)
    }

    fn require(&self, provider: AdapterProvider) -> Result<&dyn RemoteAdapter, GatewayError> {
        self.get(provider).ok_or_else(|| {
            GatewayError::new(adapter_diagnostic(
                provider,
                "adapter_unsupported",
                "The requested adapter is not registered.",
                "Register the adapter before calling connect or test-message.",
            ))
        })
    }
}

pub(crate) fn adapter_diagnostic(
    provider: AdapterProvider,
    code: impl Into<String>,
    message: impl Into<String>,
    action: impl Into<String>,
) -> GatewayDiagnostic {
    GatewayDiagnostic::new(code, message, action)
        .with_context(format!("provider={}", provider.as_str()))
}
