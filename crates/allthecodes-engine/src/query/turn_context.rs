use std::sync::Arc;

use tracing::{debug, warn};

use crate::types::config::{QueryGates, QueryParams, QuerySource};
use crate::types::state::QueryLoopState;
use crate::types::tool::Tools;
use crate::verification::policy::{policy_by_name, VerificationPolicy};
use allthecodes_session::record_replay::types::{EvidenceKind, VerificationEvidenceRecord};

use super::deps::{ModelCallParams, QueryDeps};

pub(crate) struct QueryRunContext {
    pub system_prompt: Vec<String>,
    pub max_turns: Option<usize>,
    pub task_budget_total: Option<u64>,
    pub query_source: QuerySource,
    pub skip_cache_write: Option<bool>,
    pub fallback_model: Option<String>,
    pub gates: QueryGates,
    pub verification: VerificationTracker,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct VerificationTracker {
    pub policy: Option<VerificationPolicy>,
    pub round: u8,
    pub evidence: Vec<VerificationEvidenceRecord>,
    pub failed_attempts: Vec<String>,
    pub configuration_error: Option<String>,
}

impl VerificationTracker {
    fn from_name(name: Option<String>) -> Self {
        let Some(name) = name else {
            return Self::default();
        };
        match policy_by_name(&name) {
            Ok(policy) if policy.max_rounds > 0 => Self {
                policy: Some(policy),
                round: 1,
                ..Self::default()
            },
            Ok(_) => Self::default(),
            Err(error) => {
                tracing::warn!(policy = %name, %error, "rejecting unknown verification policy");
                Self {
                    policy: Some(VerificationPolicy {
                        name: name.clone(),
                        required: Vec::new(),
                        max_rounds: 1,
                    }),
                    round: 1,
                    failed_attempts: vec![format!("invalid verification policy: {name}")],
                    configuration_error: Some(error.to_string()),
                    ..Self::default()
                }
            }
        }
    }

    pub(crate) fn missing_names(kinds: &[EvidenceKind]) -> String {
        kinds
            .iter()
            .map(|kind| match kind {
                EvidenceKind::Build => "build",
                EvidenceKind::Test => "test",
                EvidenceKind::Lint => "lint",
                EvidenceKind::SecurityGate => "security_gate",
                EvidenceKind::ApiSmoke => "api_smoke",
                EvidenceKind::BrowserSmoke => "browser_smoke",
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl QueryRunContext {
    pub(crate) fn from_params(params: QueryParams) -> (Self, QueryLoopState) {
        let mut state = QueryLoopState::initial(params.messages);
        state.max_output_tokens_override = params.max_output_tokens_override;

        let context = Self {
            system_prompt: params.system_prompt,
            max_turns: params.max_turns,
            task_budget_total: params.task_budget.as_ref().map(|budget| budget.total),
            query_source: params.query_source,
            skip_cache_write: params.skip_cache_write,
            fallback_model: Some(
                params
                    .fallback_model
                    .unwrap_or_else(allthecodes_types::models::default_fallback_model_id),
            ),
            gates: params.gates,
            verification: VerificationTracker::from_name(params.verification_policy),
        };

        (context, state)
    }

    pub(crate) fn token_budget_scope(&self) -> Option<&'static str> {
        if self.query_source.starts_with_agent() {
            Some("agent")
        } else {
            None
        }
    }
}

pub(crate) struct PreparedModelRequest {
    pub tools: Tools,
    pub call_params: ModelCallParams,
}

pub(crate) async fn prepare_model_request(
    deps: &Arc<dyn QueryDeps>,
    state: &mut QueryLoopState,
    context: &QueryRunContext,
) -> PreparedModelRequest {
    let messages = match deps.microcompact(state.messages.clone()).await {
        Ok(msgs) => msgs,
        Err(error) => {
            warn!(error = %error, "microcompact failed, using original messages");
            state.messages.clone()
        }
    };

    let message_count_before = messages.len();
    run_compact_hook(
        deps,
        "PreCompact",
        serde_json::json!({
            "message_count": message_count_before,
        }),
    )
    .await;

    match deps.refresh_tools().await {
        Ok(_refreshed) => {
            debug!("tools refreshed successfully before context and model call");
        }
        Err(error) => {
            debug!(error = %error, "tool refresh failed before context and model call, continuing with existing tools");
        }
    }

    let app_state_for_request = deps.get_app_state();
    let request_model = app_state_for_request.main_loop_model.clone();
    let request_thinking_enabled = app_state_for_request.thinking_enabled;
    let request_effort_value = app_state_for_request.effort_value.clone();
    let request_output_config = app_state_for_request.settings.output_config.clone();
    let request_model_reasoning_effort = app_state_for_request
        .settings
        .model_reasoning_effort
        .clone();
    let request_advisor_model = app_state_for_request.advisor_model.clone();

    // Provider-aware effort resolution: pick the wire transport (Anthropic
    // fixed budget vs. Codex reasoning.effort vs. passthrough) and the
    // canonical level following the fixed priority chain described in
    // `engine::effort::resolve_effort`. This avoids the legacy
    // cross-contamination where Codex paths wrote `output_config.effort`
    // (Anthropic compat) and silently down-ranked per-turn overrides.
    let request_resolved_effort = resolve_request_effort(
        &app_state_for_request,
        request_effort_value.as_deref(),
        request_output_config.as_ref(),
        request_model_reasoning_effort.as_deref(),
    );
    let capability_filtered_tools = allthecodes_tools::media::filter_tools_for_model_capabilities(
        deps.get_tools(),
        &app_state_for_request.settings,
        &request_model,
    );
    let settings_filtered_tools = allthecodes_tools::registry::filter_tools_for_runtime_settings(
        capability_filtered_tools,
        &app_state_for_request.settings,
    );
    let enabled_tools =
        allthecodes_tools::registry::filter_tools_for_enabled_state(settings_filtered_tools);
    let session_filtered_tools = allthecodes_tools::registry::dedupe_tools_by_name(
        allthecodes_tools::registry::filter_tools_for_session_gates(
            enabled_tools,
            allthecodes_tools::registry::ToolSessionGates {
                non_interactive: context.query_source.is_non_interactive(),
                subagent: context.query_source.starts_with_agent(),
            },
        ),
    );
    let deferred_session_id = context
        .gates
        .deferred_tool_loading
        .then(|| deps.audit_context().session_id);
    let tools_for_request = if let Some(session_id) = deferred_session_id.as_deref() {
        allthecodes_tools::deferred_tools::filter_tools_for_deferred_request(
            session_filtered_tools,
            &messages,
            session_id,
        )
    } else {
        session_filtered_tools
    };
    let tools_for_request = allthecodes_tools::registry::dedupe_tools_by_name(tools_for_request);

    let autocompact_params = ModelCallParams {
        messages: messages.clone(),
        system_prompt: context.system_prompt.clone(),
        tools: tools_for_request.clone(),
        model: Some(request_model.clone()),
        max_output_tokens: state.max_output_tokens_override,
        skip_cache_write: context.skip_cache_write,
        thinking_enabled: request_thinking_enabled,
        effort_value: request_effort_value.clone(),
        output_config: request_output_config.clone(),
        model_reasoning_effort: request_model_reasoning_effort.clone(),
        resolved_effort: request_resolved_effort.clone(),
        advisor_model: request_advisor_model.clone(),
    };

    let (mut messages, auto_compact_tracking) = match deps
        .autocompact(autocompact_params, state.auto_compact_tracking.clone())
        .await
    {
        Ok(Some(result)) => {
            debug!("autocompact produced compacted messages");
            let message_count_after = result.messages.len();
            run_compact_hook(
                deps,
                "PostCompact",
                serde_json::json!({
                    "message_count_before": message_count_before,
                    "message_count_after": message_count_after,
                    "messages_freed": message_count_before.saturating_sub(message_count_after),
                }),
            )
            .await;

            (result.messages, Some(result.tracking))
        }
        Ok(None) => (messages, state.auto_compact_tracking.clone()),
        Err(error) => {
            warn!(error = %error, "autocompact failed, using original messages");
            (messages, state.auto_compact_tracking.clone())
        }
    };

    if let Some(session_id) = deferred_session_id.as_deref() {
        let discovered =
            allthecodes_tools::deferred_tools::discovered_tools_for_session(session_id);
        allthecodes_tools::deferred_tools::annotate_compact_boundaries_with_discovered_tools(
            &mut messages,
            &discovered,
        );
    }

    state.messages = messages;
    state.auto_compact_tracking = auto_compact_tracking;

    let call_params = ModelCallParams {
        messages: state.messages.clone(),
        system_prompt: context.system_prompt.clone(),
        tools: tools_for_request.clone(),
        model: Some(request_model),
        max_output_tokens: state.max_output_tokens_override,
        skip_cache_write: context.skip_cache_write,
        thinking_enabled: request_thinking_enabled,
        effort_value: request_effort_value,
        output_config: request_output_config,
        model_reasoning_effort: request_model_reasoning_effort,
        resolved_effort: request_resolved_effort,
        advisor_model: request_advisor_model,
    };

    PreparedModelRequest {
        tools: tools_for_request,
        call_params,
    }
}

/// Resolve the provider-aware [`ResolvedEffort`] for a turn from `AppState`.
///
/// `effort_value` carries the per-turn `SubmitMessageOverrides.effort`
/// (already merged into `app_state.effort_value` by the deps layer) plus
/// the `/effort`-set legacy value; it is treated as the priority candidate
/// once the explicit per-turn override slot is reserved. `output_config` is
/// the raw `output_config.effort` Anthropic-compat wire field.
fn resolve_request_effort(
    app_state: &crate::types::app_state::AppState,
    effort_value: Option<&str>,
    output_config: Option<&serde_json::Value>,
    model_reasoning_effort: Option<&str>,
) -> Option<crate::effort::ResolvedEffort> {
    use crate::effort::{resolve_effort, CapabilityProvenance, ResolveEffortInput};
    use allthecodes_config::settings::{codex_model_capabilities, codex_model_ids};

    let settings = &app_state.settings;
    let api_provider = settings.api_provider.as_deref();
    let effort_level = settings.effort_level.as_deref();
    let output_config_effort = output_config.and_then(crate::effort::normalize_output_effort_json);

    // Active profile's persisted Codex baseline.
    let profile_model_reasoning_effort = settings
        .active_auth_profile
        .as_deref()
        .and_then(|name| settings.auth_profiles.get(name))
        .and_then(|profile| profile.model_reasoning_effort.as_deref())
        .or(model_reasoning_effort);

    // Capability lookup: bundled catalog takes priority for provenance, but
    // we honor a user override if the model id is not in the bundled catalog.
    let model = &app_state.main_loop_model;
    let bundled_ids = codex_model_ids();
    // Lookup the model's capability and the provenance of that lookup. The
    // bundled catalog is preferred for bundled model ids (so codex
    // catalog changes keep working without a user re-login); a user
    // override is only consulted for non-bundled models or as a fallback
    // if a bundled id somehow lost its catalog entry.
    let is_bundled = bundled_ids.iter().any(|id| id == model);
    let capability: Option<allthecodes_config::settings::ModelCapabilitySettings> = if is_bundled {
        codex_model_capabilities()
            .get(model)
            .cloned()
            .or_else(|| settings.model_capabilities.get(model).cloned())
    } else {
        settings.model_capabilities.get(model).cloned()
    };
    let capability_provenance = match &capability {
        Some(_) if is_bundled => CapabilityProvenance::Bundled,
        Some(_) => CapabilityProvenance::UserProfile,
        None => CapabilityProvenance::None,
    };

    let resolved = resolve_effort(ResolveEffortInput {
        per_turn_override: None, // effort_value already carries the override.
        profile_model_reasoning_effort,
        output_config_effort: output_config_effort.as_deref(),
        effort_level,
        effort_value,
        api_provider,
        capability: capability.as_ref(),
        capability_provenance,
    });

    Some(resolved)
}

async fn run_compact_hook(deps: &Arc<dyn QueryDeps>, event: &str, payload: serde_json::Value) {
    let hooks_map = deps.get_app_state().hooks;
    let runner = deps.hook_runner();
    let configs = runner.load_hook_configs(&hooks_map, event);
    if !configs.is_empty() {
        let _ = runner.run_event_hooks(event, &payload, &configs).await;
    }
}
