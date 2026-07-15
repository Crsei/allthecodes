use std::sync::Arc;

use anyhow::Context;
use tracing::warn;

use crate::startup::settings_runtime::SettingsRuntime;
use crate::startup::startup_context::StartupContext;
use crate::startup_model::{resolve_model_alias_for_effective_settings, resolve_startup_model};

pub(crate) struct ModelRuntime {
    pub(crate) model: String,
    pub(crate) fallback_model: String,
    pub(crate) detected_client: Option<Arc<allthecodes_api::api::client::ApiClient>>,
}

pub(crate) struct ModelRuntimeBuilder;

impl ModelRuntimeBuilder {
    pub(crate) async fn build(
        startup: &StartupContext,
        settings: &SettingsRuntime,
    ) -> anyhow::Result<ModelRuntime> {
        let merged_config = &settings.merged_config;
        let backend = &settings.backend;
        let is_codex_backend = allthecodes_engine::codex_exec::is_codex_backend(backend);
        let detected_client =
            allthecodes_api::api::client::ApiClient::from_backend_result(Some(backend))
                .context("invalid API provider configuration")?
                .map(Arc::new);
        let provider_default_model = detected_client
            .as_ref()
            .map(|client| client.config().default_model.clone());

        if detected_client.is_none() {
            if is_codex_backend {
                warn!("No OpenAI Codex auth detected. Set OPENAI_CODEX_AUTH_TOKEN.");
                eprintln!(
                    "\x1b[33m- No OpenAI Codex auth detected.\x1b[0m\n  \
                     Set:\n  \
                     - OPENAI_CODEX_AUTH_TOKEN (required)\n  \
                     - OPENAI_CODEX_BASE_URL (optional, default: https://chatgpt.com/backend-api)\n  \
                     - OPENAI_CODEX_MODEL (optional, default: gpt-5.6-sol)"
                );
            } else {
                warn!(
                    "No API provider detected. Set an API key in .env, environment, or use /login."
                );
                eprintln!(
                    "\x1b[33m- No API provider detected.\x1b[0m\n  \
                     Set an API key via:\n  \
                     - .env file (ANTHROPIC_API_KEY, AZURE_API_KEY, OPENAI_API_KEY, ...)\n  \
                     - Environment variable\n  \
                     - /login command in the REPL"
                );
            }
        }

        let hardcoded_default = if let Some(default_model) = merged_config.default_model.as_deref()
        {
            default_model.to_string()
        } else if is_codex_backend {
            allthecodes_engine::codex_exec::DEFAULT_CODEX_MODEL.to_string()
        } else {
            allthecodes_types::models::DEFAULT_MODEL_ALIAS.to_string()
        };
        let requested_model = startup.cli.model.clone().or(merged_config.model.clone());
        let model = resolve_startup_model(
            requested_model.as_deref(),
            provider_default_model.as_deref(),
            &hardcoded_default,
            &merged_config.available_models,
            merged_config,
        );
        let fallback_model = merged_config
            .fallback_model
            .as_deref()
            .map(|model| resolve_model_alias_for_effective_settings(model, merged_config))
            .unwrap_or_else(|| {
                let is_anthropic_compatible = detected_client.as_ref().is_some_and(|client| {
                    matches!(
                        client.config().provider.endpoint_kind(),
                        Some(
                            allthecodes_api::api::providers::AnthropicEndpointKind::CompatibleAnthropic
                        )
                    )
                });
                if is_anthropic_compatible {
                    model.clone()
                } else {
                    resolve_model_alias_for_effective_settings(
                        allthecodes_types::models::DEFAULT_FALLBACK_MODEL_ALIAS,
                        merged_config,
                    )
                }
            });

        Ok(ModelRuntime {
            model,
            fallback_model,
            detected_client,
        })
    }
}
