use super::*;

pub(crate) fn tool_execution_result_to_exec_result(result: ToolExecutionResult) -> ToolExecResult {
    let mut tool_result = result.result;
    if !result.new_messages.is_empty() {
        tool_result.new_messages = result.new_messages;
    }

    ToolExecResult {
        tool_use_id: result.tool_use_id,
        tool_name: result.tool_name,
        result: tool_result,
        is_error: result.is_error,
        hook_stopped_continuation: result.hook_stopped_continuation,
    }
}

pub(crate) fn merge_refreshed_mcp_tools(
    existing_tools: Tools,
    refreshed_mcp_tools: Tools,
) -> Tools {
    let mut seen = HashSet::new();
    let mut merged = Vec::with_capacity(existing_tools.len() + refreshed_mcp_tools.len());

    for tool in existing_tools {
        if tool.mcp_server_name().is_some() {
            continue;
        }
        let name = tool.name().to_string();
        if seen.insert(name.clone()) {
            merged.push(tool);
        } else {
            tracing::warn!(tool = %name, "skipping existing tool with duplicate name");
        }
    }

    for tool in refreshed_mcp_tools {
        let name = tool.name().to_string();
        if seen.insert(name.clone()) {
            merged.push(tool);
        } else {
            tracing::warn!(tool = %name, "skipping refreshed MCP tool with duplicate name");
        }
    }

    merged
}

pub(crate) fn prepare_model_call_params_for_client(
    params: &mut ModelCallParams,
    app_model: &str,
    client: &allthecodes_api::api::client::ApiClient,
) {
    if params.model.as_deref().unwrap_or_default().is_empty() {
        params.model = Some(if app_model.is_empty() {
            client.config().default_model.clone()
        } else {
            app_model.to_string()
        });
    }

    if !allthecodes_api::api::client::provider_supports_advisor(&client.config().provider)
        && params.advisor_model.is_some()
    {
        tracing::debug!(
            provider = client.langfuse_provider_name(),
            "dropping advisor_model - provider does not support it"
        );
        params.advisor_model = None;
    }
}

pub(crate) fn model_for_autocompact(
    params: &mut ModelCallParams,
    app_model: &str,
    client: Option<&allthecodes_api::api::client::ApiClient>,
) -> String {
    if let Some(client) = client {
        prepare_model_call_params_for_client(params, app_model, client);
        return params
            .model
            .clone()
            .unwrap_or_else(|| client.config().default_model.clone());
    }

    let model = if let Some(model) = params.model.as_deref().filter(|model| !model.is_empty()) {
        model.to_string()
    } else if app_model.is_empty() {
        allthecodes_models::default_fallback_model_id()
    } else {
        app_model.to_string()
    };
    params.model = Some(model.clone());
    model
}

pub(crate) fn record_request_snapshot(
    session_id: &str,
    provider: &str,
    request: &allthecodes_api::api::client::MessagesRequest,
) {
    let value = match serde_json::to_value(request) {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%error, "failed to serialize API request snapshot");
            return;
        }
    };
    if let Err(error) = allthecodes_session::request_snapshot::record_api_request_snapshot(
        session_id, provider, &value,
    ) {
        tracing::warn!(session_id, %provider, %error, "failed to record API request snapshot");
    }
}

impl QueryEngineDeps {
    pub(crate) async fn call_model_impl(
        &self,
        mut params: ModelCallParams,
    ) -> Result<ModelResponse> {
        let client = self.api_client.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "call_model: no API client configured -- \
                 set a provider key (ANTHROPIC_API_KEY / OPENAI_API_KEY / OPENAI_CODEX_AUTH_TOKEN), \
                 use /login for Anthropic, or provide a mock in tests"
            )
        })?;

        let app_model = self.get_app_state().main_loop_model;
        prepare_model_call_params_for_client(&mut params, &app_model, client);

        // Strip advisor_model for providers that don't support it (issue #33).
        if !allthecodes_api::api::client::provider_supports_advisor(&client.config().provider)
            && params.advisor_model.is_some()
        {
            tracing::debug!(
                provider = client.langfuse_provider_name(),
                "dropping advisor_model — provider does not support it"
            );
            params.advisor_model = None;
        }

        let request = build_messages_request(&params);
        record_request_snapshot(&self.session_id, client.langfuse_provider_name(), &request);
        let stream = client.messages_stream(request).await?;
        let mut stream = std::pin::pin!(stream);

        let mut accumulator = allthecodes_api::api::streaming::StreamAccumulator::new();
        let mut stream_events = Vec::new();

        use futures::StreamExt;
        while let Some(event_result) = stream.next().await {
            let event = event_result?;
            accumulator.process_event(&event);
            stream_events.push(event);
        }

        let usage = accumulator.usage.clone();
        let model_id = params.model.as_deref().unwrap_or("unknown");
        let assistant_message = accumulator.build(model_id);

        Ok(ModelResponse {
            assistant_message,
            stream_events,
            usage,
        })
    }

    pub(crate) async fn call_model_streaming_impl(
        &self,
        mut params: ModelCallParams,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let client = self.api_client.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "call_model_streaming: no API client configured -- \
                 set a provider key (ANTHROPIC_API_KEY / OPENAI_API_KEY / OPENAI_CODEX_AUTH_TOKEN), \
                 use /login for Anthropic, or provide a mock in tests"
            )
        })?;

        let app_model = self.get_app_state().main_loop_model;
        prepare_model_call_params_for_client(&mut params, &app_model, client);

        // Strip advisor_model for providers that don't support it (issue #33).
        if !allthecodes_api::api::client::provider_supports_advisor(&client.config().provider)
            && params.advisor_model.is_some()
        {
            tracing::debug!(
                provider = client.langfuse_provider_name(),
                "dropping advisor_model — provider does not support it"
            );
            params.advisor_model = None;
        }

        let request = build_messages_request(&params);
        record_request_snapshot(&self.session_id, client.langfuse_provider_name(), &request);
        if (allthecodes_api::api::client::is_env_truthy("ALLTHECODES_EXACT_TOKEN_DIAGNOSTICS")
            || allthecodes_api::api::client::is_env_truthy("CC_RUST_EXACT_TOKEN_DIAGNOSTICS"))
            && client.supports_exact_token_count()
        {
            match client.count_token_usage_exact(&request).await {
                Ok(report) => {
                    tracing::debug!(
                        provider = report.provider.as_deref().unwrap_or("unknown"),
                        input_tokens = report.estimated_tokens,
                        context_window = report.context_window,
                        threshold_tokens = report.threshold_tokens,
                        over_threshold = report.over_threshold,
                        "provider exact token diagnostics"
                    );
                }
                Err(error) => {
                    tracing::debug!(
                        %error,
                        "provider exact token diagnostics unavailable; continuing with request"
                    );
                }
            }
        }
        client.messages_stream(request).await
    }
    pub(crate) async fn refresh_tools_impl(&self) -> Result<Tools> {
        let Some(manager) = allthecodes_mcp::runtime::current_manager() else {
            return Ok(self.state.read().tools.clone());
        };

        let mcp_tool_defs = {
            let manager_guard = manager.lock().await;
            manager_guard.all_tools()
        };
        let mcp_tools = crate::mcp_tool_adapter::mcp_tools_to_tools(mcp_tool_defs, manager);

        let refreshed = {
            let mut state = self.state.write();
            let refreshed = merge_refreshed_mcp_tools(state.tools.clone(), mcp_tools);
            state.tools = refreshed.clone();
            refreshed
        };

        crate::tool_runtime::tool_search::install_runtime_tool_catalog(&refreshed);
        Ok(refreshed)
    }
}
