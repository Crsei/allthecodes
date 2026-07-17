use super::*;

fn auto_compact_trigger_tracking(tracking: Option<&AutoCompactTracking>) -> AutoCompactTracking {
    let base = tracking.cloned().unwrap_or(AutoCompactTracking {
        compacted: false,
        turn_counter: 0,
        turn_id: String::new(),
        consecutive_failures: 0,
    });

    AutoCompactTracking {
        compacted: true,
        turn_counter: base.turn_counter + 1,
        turn_id: base.turn_id,
        consecutive_failures: base.consecutive_failures,
    }
}

fn exact_auto_compact_triggered_with_threshold(
    heuristic_triggered: bool,
    exact_report: Option<&allthecodes_utils::tokens::TokenUsageReport>,
    threshold_tokens: u64,
) -> bool {
    exact_report.map_or(heuristic_triggered, |report| {
        report.estimated_tokens > threshold_tokens
    })
}

fn compact_pipeline_config_from_state(
    state: &crate::types::app_state::AppState,
) -> crate::compact::pipeline::PipelineConfig {
    let threshold = state
        .settings
        .compact_threshold
        .filter(|value| (1..=100).contains(value))
        .unwrap_or_else(crate::compact::auto_compact::default_auto_compact_threshold_percent);

    crate::compact::pipeline::PipelineConfig {
        auto_compact: state.settings.auto_compact.unwrap_or(true),
        compact_threshold_percent: threshold,
        keep_recent_messages: state.settings.keep_recent_messages.unwrap_or(200) as usize,
    }
}

pub(crate) fn build_auto_compact_exact_count_request(
    base_params: &ModelCallParams,
    messages: Vec<Message>,
    model: &str,
) -> allthecodes_api::api::client::MessagesRequest {
    let mut count_params = base_params.clone();
    count_params.messages = messages;
    count_params.model = Some(model.to_string());
    count_params.skip_cache_write = Some(true);
    build_messages_request(&count_params)
}

impl QueryEngineDeps {
    pub(crate) async fn microcompact_impl(&self, messages: Vec<Message>) -> Result<Vec<Message>> {
        let result = crate::compact::microcompact::microcompact_messages(messages);
        if result.tokens_freed > 0 {
            tracing::debug!(
                tokens_freed = result.tokens_freed,
                "microcompact: trimmed old tool results"
            );
        }
        Ok(result.messages)
    }

    pub(crate) async fn autocompact_impl(
        &self,
        mut params: ModelCallParams,
        tracking: Option<AutoCompactTracking>,
    ) -> Result<Option<CompactionResult>> {
        let original_messages = params.messages.clone();
        let (app_model, pipeline_config) = {
            let state = self.state.read();
            (
                state.app_state.main_loop_model.clone(),
                compact_pipeline_config_from_state(&state.app_state),
            )
        };
        let model = model_for_autocompact(
            &mut params,
            &app_model,
            self.api_client.as_ref().map(|client| client.as_ref()),
        );

        // Run the local context pipeline (budget -> snip -> microcompact -> auto-compact check)
        let pipeline_result = crate::compact::pipeline::run_context_pipeline_with_config(
            original_messages.clone(),
            tracking.clone(),
            &model,
            pipeline_config,
        )
        .await;

        let mut auto_compact_triggered = pipeline_result.auto_compact_triggered;
        let mut auto_compact_tracking = pipeline_result.tracking.clone();

        if pipeline_config.auto_compact
            && crate::compact::auto_compact::should_check_exact_for_auto_compact_with_percent(
                pipeline_result.auto_compact_estimated_tokens,
                &model,
                pipeline_config.compact_threshold_percent,
            )
        {
            if let Some(client) = self
                .api_client
                .as_ref()
                .filter(|client| client.supports_exact_token_count())
            {
                let count_request = build_auto_compact_exact_count_request(
                    &params,
                    pipeline_result.messages.clone(),
                    &model,
                );
                match client.count_token_usage_exact(&count_request).await {
                    Ok(report) => {
                        let threshold_tokens =
                            crate::compact::auto_compact::auto_compact_threshold_tokens_for_percent(
                                &model,
                                pipeline_config.compact_threshold_percent,
                            );
                        let exact_triggered = exact_auto_compact_triggered_with_threshold(
                            auto_compact_triggered,
                            Some(&report),
                            threshold_tokens,
                        );
                        tracing::debug!(
                            provider = report.provider.as_deref().unwrap_or("unknown"),
                            exact_tokens = report.estimated_tokens,
                            threshold_tokens = threshold_tokens,
                            heuristic_tokens = pipeline_result.auto_compact_estimated_tokens,
                            heuristic_triggered = auto_compact_triggered,
                            exact_triggered = exact_triggered,
                            "auto-compact exact threshold check"
                        );
                        auto_compact_triggered = exact_triggered;
                        auto_compact_tracking = if auto_compact_triggered {
                            Some(auto_compact_trigger_tracking(tracking.as_ref()))
                        } else {
                            tracking.clone()
                        };
                    }
                    Err(error) => {
                        tracing::debug!(
                            %error,
                            heuristic_tokens = pipeline_result.auto_compact_estimated_tokens,
                            "auto-compact exact threshold check unavailable; keeping heuristic decision"
                        );
                    }
                }
            }
        }

        // If auto-compact was triggered AND we have an API client, generate a model summary
        if auto_compact_triggered {
            super::super::set_proactive_context_blocked(true, "context_limit");
            let updated_tracking = auto_compact_tracking
                .clone()
                .unwrap_or_else(|| auto_compact_trigger_tracking(tracking.as_ref()));
            let session_memory_context = {
                let state = self.state.read();
                state
                    .runtime
                    .session_memory
                    .format_memory_context_for_workspace_excluding_session(
                        5,
                        Some(std::path::Path::new(&self.cwd)),
                        Some(self.audit_ctx.session_id.as_str()),
                    )
            };
            if let Some(context) = session_memory_context.as_deref() {
                if let Some(session_memory_result) =
                    crate::compact::session_memory_compact::session_memory_compact_if_needed(
                        pipeline_result.messages.clone(),
                        context,
                    )
                {
                    tracing::info!(
                        tokens_freed = session_memory_result.tokens_freed,
                        kept_start_index = session_memory_result.kept_start_index,
                        "autocompact: session-memory summary complete"
                    );
                    let new_tracking = crate::compact::compaction::tracking_on_success(
                        tracking.as_ref(),
                        &Uuid::new_v4().to_string(),
                    );
                    super::super::set_proactive_context_blocked(false, "context_ready");
                    return Ok(Some(CompactionResult {
                        messages: session_memory_result.messages,
                        tracking: new_tracking,
                    }));
                }
            }

            // Try model-based summarization if API client is available
            if let Some(ref _client) = self.api_client {
                let summary_prompt = crate::compact::compaction::build_compaction_prompt();
                let pre_tokens = crate::utils::tokens::estimate_messages_tokens(&original_messages);

                // Build a summarization request
                let summary_messages = vec![Message::User(crate::types::message::UserMessage {
                    uuid: Uuid::new_v4(),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    role: "user".into(),
                    content: crate::types::message::MessageContent::Text(
                        format_conversation_for_summary(&original_messages),
                    ),
                    is_meta: true,
                    tool_use_result: None,
                    source_tool_assistant_uuid: None,
                })];

                let summary_params = ModelCallParams {
                    messages: summary_messages,
                    system_prompt: vec![summary_prompt],
                    tools: vec![],
                    model: Some(model.clone()),
                    max_output_tokens: Some(20_000),
                    skip_cache_write: Some(true),
                    thinking_enabled: None,
                    effort_value: None,
                    output_config: None,
                    model_reasoning_effort: None,
                    resolved_effort: None,
                    advisor_model: None,
                };

                match self.call_model(summary_params).await {
                    Ok(response) => {
                        // Extract summary text from assistant response
                        let summary_text = response
                            .assistant_message
                            .content
                            .iter()
                            .filter_map(|b| match b {
                                crate::types::message::ContentBlock::Text { text } => {
                                    Some(text.as_str())
                                }
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n");

                        let config = crate::compact::compaction::CompactionConfig {
                            model: model.clone(),
                            session_id: String::new(),
                            query_source: "compact".into(),
                        };

                        let post_messages = build_post_compact_messages_with_boundary(
                            &summary_text,
                            &original_messages,
                            &config,
                            pre_tokens,
                        );

                        let post_tokens =
                            crate::utils::tokens::estimate_messages_tokens(&post_messages);

                        tracing::info!(
                            pre_tokens = pre_tokens,
                            post_tokens = post_tokens,
                            "autocompact: model-based summary complete"
                        );

                        let new_tracking = crate::compact::compaction::tracking_on_success(
                            tracking.as_ref(),
                            &Uuid::new_v4().to_string(),
                        );
                        super::super::set_proactive_context_blocked(false, "context_ready");

                        return Ok(Some(CompactionResult {
                            messages: post_messages,
                            tracking: new_tracking,
                        }));
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "autocompact: model summary failed, using local pipeline");
                        let new_tracking =
                            crate::compact::compaction::tracking_on_failure(tracking.as_ref());
                        // Fall through to local-only result
                        super::super::set_proactive_context_blocked(false, "context_ready");
                        return Ok(Some(CompactionResult {
                            messages: pipeline_result.messages,
                            tracking: new_tracking,
                        }));
                    }
                }
            }

            // No API client -- return local pipeline result
            super::super::set_proactive_context_blocked(false, "context_ready");
            return Ok(Some(CompactionResult {
                messages: pipeline_result.messages,
                tracking: updated_tracking,
            }));
        }

        // Pipeline ran but auto-compact was not triggered -- return local compacted messages
        if pipeline_result.compacted {
            super::super::set_proactive_context_blocked(false, "context_ready");
            return Ok(Some(CompactionResult {
                messages: pipeline_result.messages,
                tracking: tracking.unwrap_or(AutoCompactTracking {
                    compacted: false,
                    turn_counter: 0,
                    turn_id: String::new(),
                    consecutive_failures: 0,
                }),
            }));
        }

        Ok(None)
    }

    pub(crate) async fn reactive_compact_impl(
        &self,
        messages: Vec<Message>,
    ) -> Result<Option<CompactionResult>> {
        let model = {
            let app = &self.state.read().app_state;
            if app.main_loop_model.is_empty() {
                self.api_client
                    .as_ref()
                    .map(|c| c.config().default_model.clone())
                    .unwrap_or_else(allthecodes_types::models::default_fallback_model_id)
            } else {
                app.main_loop_model.clone()
            }
        };

        match crate::compact::pipeline::try_reactive_compact(messages, &model).await {
            Some(result) => {
                tracing::info!(
                    tokens_freed = result.tokens_freed,
                    "reactive compact: freed tokens via emergency pipeline"
                );
                super::super::set_proactive_context_blocked(false, "context_ready");
                Ok(Some(CompactionResult {
                    messages: result.messages,
                    tracking: result.tracking,
                }))
            }
            None => Ok(None),
        }
    }

    pub(crate) async fn collapse_drain_impl(
        &self,
        messages: Vec<Message>,
        tracking: Option<AutoCompactTracking>,
    ) -> Result<Option<CompactionResult>> {
        let model = {
            let app = &self.state.read().app_state;
            if app.main_loop_model.is_empty() {
                self.api_client
                    .as_ref()
                    .map(|c| c.config().default_model.clone())
                    .unwrap_or_else(allthecodes_types::models::default_fallback_model_id)
            } else {
                app.main_loop_model.clone()
            }
        };

        let pipeline_config = {
            let state = self.state.read();
            compact_pipeline_config_from_state(&state.app_state)
        };

        let pipeline_result = crate::compact::pipeline::run_context_pipeline_with_config(
            messages,
            tracking.clone(),
            &model,
            pipeline_config,
        )
        .await;

        if !pipeline_result.compacted {
            return Ok(None);
        }

        tracing::info!(
            snip_tokens_freed = pipeline_result.snip_tokens_freed,
            microcompact_tokens_freed = pipeline_result.microcompact_tokens_freed,
            context_collapse_tokens_freed = pipeline_result.context_collapse_tokens_freed,
            total_tokens_freed = pipeline_result.total_tokens_freed,
            "collapse drain: committed local context pipeline result"
        );
        super::super::set_proactive_context_blocked(false, "context_ready");

        Ok(Some(CompactionResult {
            messages: pipeline_result.messages,
            tracking: pipeline_result.tracking.unwrap_or_else(|| {
                tracking.unwrap_or(AutoCompactTracking {
                    compacted: false,
                    turn_counter: 0,
                    turn_id: String::new(),
                    consecutive_failures: 0,
                })
            }),
        }))
    }
}
