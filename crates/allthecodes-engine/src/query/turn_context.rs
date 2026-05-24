use std::sync::Arc;

use tracing::{debug, warn};

use crate::types::config::{QueryGates, QueryParams, QuerySource};
use crate::types::state::QueryLoopState;
use crate::types::tool::Tools;

use super::deps::{ModelCallParams, QueryDeps};

pub(crate) struct QueryRunContext {
    pub system_prompt: Vec<String>,
    pub max_turns: Option<usize>,
    pub task_budget_total: Option<u64>,
    pub query_source: QuerySource,
    pub skip_cache_write: Option<bool>,
    pub fallback_model: Option<String>,
    pub gates: QueryGates,
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
                    .unwrap_or_else(allthecodes_models::default_fallback_model_id),
            ),
            gates: params.gates,
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

    let tools_for_request = deps.get_tools();
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
        advisor_model: request_advisor_model.clone(),
    };

    let (messages, auto_compact_tracking) = match deps
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
        advisor_model: request_advisor_model,
    };

    PreparedModelRequest {
        tools: tools_for_request,
        call_params,
    }
}

async fn run_compact_hook(deps: &Arc<dyn QueryDeps>, event: &str, payload: serde_json::Value) {
    let hooks_map = deps.get_app_state().hooks;
    let runner = deps.hook_runner();
    let configs = runner.load_hook_configs(&hooks_map, event);
    if !configs.is_empty() {
        let _ = runner.run_event_hooks(event, &payload, &configs).await;
    }
}
