use tracing::{info, warn};

use crate::startup::settings_runtime::SettingsRuntime;
use crate::startup::startup_context::StartupContext;

pub(crate) struct PluginRuntime {
    pub(crate) all_plugins: Vec<allthecodes_plugins::PluginEntry>,
}

pub(crate) struct PluginRuntimeBuilder;

impl PluginRuntimeBuilder {
    pub(crate) async fn build(
        _startup: &StartupContext,
        _settings: &SettingsRuntime,
    ) -> anyhow::Result<PluginRuntime> {
        allthecodes_plugins::init_plugins();
        let all_plugins = allthecodes_plugins::get_all_plugins();
        if !all_plugins.is_empty() {
            info!(count = all_plugins.len(), "plugins loaded");
        }
        allthecodes_web::state::install_plugin_runtime_hooks();
        match allthecodes_session::worktree_sessions::reconcile_all_worktree_sessions() {
            Ok(orphaned) if orphaned > 0 => {
                info!(
                    orphaned,
                    "worktree session startup reconciliation completed"
                );
            }
            Ok(_) => {}
            Err(error) => {
                warn!(%error, "worktree session startup reconciliation failed");
            }
        }

        allthecodes_lsp_service::set_recommendation_engine(
            allthecodes_lsp_service::recommendation::RecommendationEngine::from_builtin(),
        );
        {
            let enabled_plugins = allthecodes_plugins::get_enabled_plugins();
            let lsp_decls =
                allthecodes_plugins::lsp::collect_plugin_lsp_declarations(&enabled_plugins);
            if !lsp_decls.is_empty() {
                let provider_configs: Vec<allthecodes_lsp_service::LspServerConfig> = lsp_decls
                    .into_iter()
                    .map(|decl| allthecodes_lsp_service::LspServerConfig {
                        name: Some(format!("{}:{}", decl.plugin_id, decl.language)),
                        language_id: decl.language,
                        extensions: decl.extensions,
                        extension_to_language: std::collections::HashMap::new(),
                        command: decl.server_command,
                        args: decl.args,
                        env: std::collections::HashMap::new(),
                        workspace_folder: None,
                        init_options: decl.config,
                        source: Some(format!("plugin:{}", decl.plugin_id)),
                    })
                    .collect();
                if !provider_configs.is_empty() {
                    allthecodes_lsp_service::set_config_provider(Some(std::sync::Arc::new(
                        move || provider_configs.clone(),
                    )));
                }
            }
        }

        {
            use allthecodes_commands::dynamic_registry::{CommandSource, DynamicCommandEntry};
            for plugin in &all_plugins {
                if !matches!(plugin.status, allthecodes_plugins::PluginStatus::Installed) {
                    continue;
                }
                if let Some(ref cache_path) = plugin.cache_path {
                    if let Ok(manifest) = allthecodes_plugins::manifest::load_manifest(cache_path) {
                        for cmd in manifest.commands {
                            allthecodes_commands::DYNAMIC_REGISTRY.lock().register(
                                DynamicCommandEntry {
                                    name: cmd.name,
                                    aliases: cmd.aliases,
                                    description: cmd.description,
                                    source: CommandSource::Plugin,
                                    plugin_id: Some(plugin.id.clone()),
                                    hidden: false,
                                    usage_score: 0.0,
                                    execution_strategy:
                                        allthecodes_commands::dynamic_registry::ExecutionStrategy::Plugin,
                                },
                            );
                        }
                    }
                }
            }
        }

        #[cfg(feature = "telemetry")]
        {
            use allthecodes_engine::telemetry_bridge::{self, EngineTelemetry, SpanId};
            use allthecodes_services::telemetry::{
                init_telemetry, TelemetryConfig, TelemetryExporter, TelemetryHandle,
                TelemetryRedaction,
            };
            use std::sync::atomic::{AtomicU64, Ordering};
            use std::sync::Mutex;

            let telemetry_config = TelemetryConfig {
                enabled: true,
                exporter: TelemetryExporter::Log,
                sampling_rate: 1.0,
                redaction: TelemetryRedaction::default(),
            };
            let telemetry_handle = init_telemetry(telemetry_config);
            info!("telemetry subsystem initialized");

            struct EngineTelemetryBridge {
                handle: TelemetryHandle,
                span_counter: AtomicU64,
                active_spans: Mutex<
                    std::collections::HashMap<
                        SpanId,
                        allthecodes_services::telemetry::InteractionSpan,
                    >,
                >,
                active_hooks: Mutex<
                    std::collections::HashMap<SpanId, allthecodes_services::telemetry::HookSpan>,
                >,
            }

            impl EngineTelemetry for EngineTelemetryBridge {
                fn start_submit(&self, session_id: &str, submit_id: &str) -> SpanId {
                    let id = self.span_counter.fetch_add(1, Ordering::Relaxed);
                    let span = self
                        .handle
                        .start_interaction(session_id.to_string(), submit_id.to_string());
                    self.active_spans
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .insert(id, span);
                    id
                }

                fn end_submit(
                    &self,
                    span_id: SpanId,
                    model: &str,
                    input_tokens: u64,
                    output_tokens: u64,
                ) {
                    if let Some(mut span) = self
                        .active_spans
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .remove(&span_id)
                    {
                        span.finish(model, input_tokens as u32, output_tokens as u32);
                    }
                }

                fn start_hook(&self, hook_name: &str) -> SpanId {
                    let id = self.span_counter.fetch_add(1, Ordering::Relaxed);
                    let span = allthecodes_services::telemetry::HookSpan::start(
                        hook_name.to_string(),
                        self.handle.clone(),
                    );
                    self.active_hooks
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .insert(id, span);
                    id
                }

                fn end_hook(&self, span_id: SpanId, _result: &str) {
                    if let Some(mut span) = self
                        .active_hooks
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .remove(&span_id)
                    {
                        if _result == "error" {
                            span.record_error("hook returned error");
                        } else {
                            span.finish();
                        }
                    }
                }

                fn record_verification_report(
                    &self,
                    session_id: &str,
                    policy: &str,
                    status: &str,
                    rounds: u8,
                    evidence_count: u64,
                    report_integrity_valid: Option<bool>,
                    cost_usd: Option<f64>,
                ) {
                    let tracer =
                        allthecodes_services::telemetry::session_tracing::SessionTracer::new(
                            session_id,
                            allthecodes_services::telemetry::TelemetryConfig::default(),
                            Some(self.handle.clone()),
                        );
                    tracer.trace_verification_report(
                        &allthecodes_services::telemetry::session_tracing::VerificationReportMetadata {
                            policy: policy.to_string(),
                            status: status.to_string(),
                            rounds,
                            evidence_count,
                            report_integrity_valid,
                            cost_usd,
                        },
                    );
                }
            }

            let bridge = EngineTelemetryBridge {
                handle: telemetry_handle.clone(),
                span_counter: AtomicU64::new(1),
                active_spans: Mutex::new(std::collections::HashMap::new()),
                active_hooks: Mutex::new(std::collections::HashMap::new()),
            };
            telemetry_bridge::install(Box::new(bridge))
                .map_err(|_| anyhow::anyhow!("telemetry bridge already installed"))?;
            info!("telemetry bridge installed");
        }

        Ok(PluginRuntime { all_plugins })
    }
}
