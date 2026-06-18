//! Provider handlers — list, CRUD, Codex OAuth, probes, and model helpers.

mod api_proxy;
mod codex;
mod crud;
mod helpers;
mod internal_api;
mod probe;
mod types;

pub use api_proxy::{
    anthropic_proxy_count_tokens_handler, anthropic_proxy_messages_handler,
    openai_proxy_responses_handler,
};
pub use codex::{codex_apply_local_handler, codex_local_status_handler};
pub use crud::{
    providers_create_handler, providers_delete_handler, providers_list_handler,
    providers_refresh_models_handler, providers_update_handler,
};
pub(crate) use internal_api::{
    configured_provider_models, provider_for_model, update_configured_model,
};
pub use probe::providers_probe_handler;
pub use types::{
    CodexApplyLocalResponse, CodexLocalStatusResponse, ModelDiscoveryResponse,
    ProviderCreateRequest, ProviderListResponse, ProviderPreset, ProviderSummary,
    ProviderUpdateRequest,
};

#[cfg(test)]
mod tests;
