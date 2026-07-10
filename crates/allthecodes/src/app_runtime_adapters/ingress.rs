//! Frontend message dispatch (ingress).
//!
//! Receives parsed [`FrontendMessage`]s and dispatches them to the appropriate
//! handler or runtime action.  Slash command execution also lives here.

use std::sync::Arc;

use parking_lot::Mutex;
use tracing::{debug, warn};

use allthecodes_engine::command_runtime::{CommandContext, CommandResult};
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_services::prompt_suggestion::PromptSuggestionService;
use allthecodes_types::commands::CommandDispatcher;
use allthecodes_types::message::{ContentBlock, Message, MessageContent};

use super::sdk_mapper::handle_sdk_message;
use crate::ui::completions::{
    CombinedCompleter, CommandCompletionProvider, CompletionContext, CompletionItem, CompletionKind,
};
use crate::ui::path_completion::PathCompletionProvider;
use crate::ui::shell_history_completion::ShellHistoryCompletionProvider;
use crate::ui::slack_channel_completion::SlackChannelCompletionProvider;
use allthecodes_ipc::runtime::SessionRuntime;
use allthecodes_ipc::transport::FrontendSink;
use allthecodes_ipc_protocol::protocol::{
    CompletionItemDTO, InstallProgress, LspRecommendationDTO,
};
use allthecodes_ipc_protocol::{BackendMessage, ConversationMessage, FrontendMessage};

// ---------------------------------------------------------------------------
// FrontendMessage dispatch
// ---------------------------------------------------------------------------

/// Dispatch a single [`FrontendMessage`].
///
/// Returns `true` if the event loop should continue, `false` to break.
pub(crate) async fn dispatch(
    msg: FrontendMessage,
    engine: &Arc<QueryEngine>,
    runtime: &SessionRuntime,
    suggestion_svc: &Arc<Mutex<PromptSuggestionService>>,
    sink: &FrontendSink,
) -> bool {
    super::ensure_installed();

    match msg {
        FrontendMessage::SubmitPrompt { text, id } => {
            debug!("headless: submit_prompt id={}", id);

            if let Some(question_id) = runtime
                .pending_interactions()
                .try_answer_any_question(text.clone())
            {
                debug!(
                    "headless: routed submit_prompt to pending AskUserQuestion id={}",
                    question_id
                );
                return true;
            }

            if runtime.current_turn().is_some() {
                let _ = sink.send(&BackendMessage::Error {
                    message: "A query turn is already running; abort or wait for it to finish."
                        .to_string(),
                    recoverable: true,
                });
                return true;
            }

            let app_state = engine.app_state();
            let classifier =
                allthecodes_commands::plan_workflow::classify_plan_entry(&text, &app_state);
            if classifier.should_enter {
                match crate::plan_workflow::enter_engine_plan_mode(
                    engine,
                    "classifier",
                    Some(&text),
                    Some(&classifier.reason),
                ) {
                    Ok(record) => {
                        let summary = allthecodes_types::plan_workflow::summarize(&record);
                        let _ = sink.send(&BackendMessage::PlanWorkflowEvent {
                            event: "classifier_entered".to_string(),
                            summary: summary.clone(),
                            record,
                        });
                        let _ = sink.send(&BackendMessage::SystemInfo {
                            text: format!(
                                "Plan mode entered by classifier: {}",
                                classifier
                                    .matched_rule
                                    .as_deref()
                                    .unwrap_or(classifier.reason.as_str())
                            ),
                            level: "info".to_string(),
                        });
                    }
                    Err(err) => {
                        let _ = sink.send(&BackendMessage::SystemInfo {
                            text: format!(
                                "Plan classifier matched, but workflow sync failed: {err}"
                            ),
                            level: "warning".to_string(),
                        });
                    }
                }
            }

            if !allthecodes_ipc::client::query_runner::spawn_managed_query_turn(
                runtime.clone(),
                engine.clone(),
                text,
                id,
                suggestion_svc.clone(),
                sink.clone(),
                handle_sdk_message,
            ) {
                let _ = sink.send(&BackendMessage::Error {
                    message: "A query turn is already running; abort or wait for it to finish."
                        .to_string(),
                    recoverable: true,
                });
            }
        }

        FrontendMessage::AbortQuery => {
            debug!("headless: abort requested");
            engine.abort();
        }

        FrontendMessage::PermissionResponse {
            tool_use_id,
            decision,
            feedback,
            session_id,
            turn_id,
        } => {
            debug!(
                "headless: permission response tool_use_id={} decision={}",
                tool_use_id, decision
            );
            if !runtime.pending_interactions().complete_permission(
                session_id.as_deref(),
                turn_id.as_deref(),
                &tool_use_id,
                allthecodes_types::callbacks::PermissionResponsePayload::new(decision, feedback),
            ) {
                warn!(
                    "headless: no pending permission for tool_use_id={}",
                    tool_use_id
                );
            }
        }

        FrontendMessage::QuestionResponse {
            id,
            text,
            session_id,
            turn_id,
        } => {
            debug!("headless: question response id={}", id);
            if !runtime.pending_interactions().complete_question(
                session_id.as_deref(),
                turn_id.as_deref(),
                &id,
                text,
            ) {
                warn!("headless: no pending question for id={}", id);
            }
        }

        FrontendMessage::SlashCommand { raw } => {
            debug!("headless: slash command: {}", raw);
            if runtime.current_turn().is_some() {
                let _ = sink.send(&BackendMessage::Error {
                    message: "Cannot run a slash command while a query turn is running."
                        .to_string(),
                    recoverable: true,
                });
                return true;
            }
            handle_slash_command(&raw, engine, runtime, suggestion_svc, sink).await;
        }

        FrontendMessage::Resize { cols, rows } => {
            debug!("headless: resize {}x{}", cols, rows);
            let mut ps = allthecodes_bootstrap::PROCESS_STATE.write();
            ps.terminal_cols = cols;
            ps.terminal_rows = rows;
        }

        FrontendMessage::Quit => {
            debug!("headless: quit requested");
            return false;
        }

        FrontendMessage::LspCommand { command } => {
            debug!("headless: LSP command: {:?}", command);
            let msgs = allthecodes_ipc::subsystem_handlers::handle_lsp_command(command);
            let _ = sink.send_many(msgs);
        }
        FrontendMessage::McpCommand { command } => {
            debug!("headless: MCP command: {:?}", command);
            let cwd = std::path::Path::new(engine.cwd());
            let msgs =
                allthecodes_ipc::subsystem_handlers::handle_mcp_command_with_runtime(command, cwd)
                    .await;
            let _ = sink.send_many(msgs);
        }
        FrontendMessage::PluginCommand { command } => {
            debug!("headless: Plugin command: {:?}", command);
            let msgs = allthecodes_ipc::subsystem_handlers::handle_plugin_command(command);
            let _ = sink.send_many(msgs);
        }
        FrontendMessage::SkillCommand { command } => {
            debug!("headless: Skill command: {:?}", command);
            let msgs = allthecodes_ipc::subsystem_handlers::handle_skill_command(command);
            let _ = sink.send_many(msgs);
        }
        FrontendMessage::IdeCommand { command } => {
            debug!("headless: IDE command: {:?}", command);
            let msgs = allthecodes_ipc::subsystem_handlers::handle_ide_command(command);
            let _ = sink.send_many(msgs);
        }
        FrontendMessage::AgentSettingsCommand { command } => {
            debug!("headless: AgentSettings command: {:?}", command);
            let msgs = allthecodes_services::agent_definitions::handle(*command);
            let _ = sink.send_many(msgs);
        }
        FrontendMessage::QuerySubsystemStatus => {
            debug!("headless: subsystem status query");
            let status = allthecodes_ipc::subsystem_handlers::build_subsystem_status_snapshot();
            let _ = sink.send(&BackendMessage::SubsystemStatus { status });
        }
        FrontendMessage::AgentCommand { command } => {
            debug!("headless: Agent command: {:?}", command);
            let msgs = allthecodes_ipc::agent_handlers::handle_agent_command(command);
            let _ = sink.send_many(msgs);
        }
        FrontendMessage::TeamCommand { command } => {
            debug!("headless: Team command: {:?}", command);
            let msgs = allthecodes_ipc::agent_handlers::handle_team_command(command);
            let _ = sink.send_many(msgs);
        }

        FrontendMessage::SearchFiles {
            request_id,
            pattern,
            cwd,
            case_insensitive,
            max_results,
        } => {
            debug!(
                "headless: search_files request_id={} pattern={:?}",
                request_id, pattern
            );
            let sink = sink.clone();
            allthecodes_services::file_search::dispatch_search(
                allthecodes_services::file_search::FileSearchRequest {
                    request_id,
                    pattern,
                    cwd,
                    case_insensitive,
                    max_results,
                },
                move |msg| {
                    let _ = sink.send(&msg);
                },
            );
        }

        FrontendMessage::RequestCompletions {
            input,
            cursor_pos,
            request_id,
        } => {
            debug!(
                "headless: request_completions id={} cursor={}",
                request_id, cursor_pos
            );
            let items = compute_headless_completions(&input, cursor_pos);
            let _ = sink.send(&BackendMessage::Completions { items, request_id });
        }

        FrontendMessage::AcceptCompletion { request_id, index } => {
            debug!(
                "headless: accept_completion id={} index={}",
                request_id, index
            );
            let _ = sink.send(&BackendMessage::SystemInfo {
                text: format!("Completion {} accepted for request {}.", index, request_id),
                level: "info".to_string(),
            });
        }

        FrontendMessage::InstallRecommendedPlugin { plugin_id } => {
            debug!("headless: install_recommended_plugin {}", plugin_id);
            let _ = sink.send(&BackendMessage::PluginInstallProgress {
                plugin_id: plugin_id.clone(),
                status: InstallProgress::Downloading,
            });
            let policy = crate::command_runtime_bridge::managed_policy_for_commands();
            let (available_plugins, all_manifests) =
                crate::command_runtime_bridge::plugin_dependency_context();
            match allthecodes_plugins::installation::install_plugin(
                &plugin_id,
                None,
                Some(env!("CARGO_PKG_VERSION")),
                policy.as_ref(),
                &available_plugins,
                &all_manifests,
            )
            .await
            {
                Ok(_) => {
                    let _ = sink.send(&BackendMessage::PluginInstallProgress {
                        plugin_id,
                        status: InstallProgress::Installed,
                    });
                }
                Err(error) => {
                    let _ = sink.send(&BackendMessage::PluginInstallProgress {
                        plugin_id,
                        status: InstallProgress::Failed {
                            error: error.to_string(),
                        },
                    });
                }
            }
        }

        FrontendMessage::RefreshPluginTelemetry => {
            debug!("headless: refresh_plugin_telemetry");
            let _ = sink.send(&BackendMessage::TelemetryStatus {
                enabled: telemetry_bridge_active(),
                session_id: Some(engine.current_session_id().to_string()),
                errors: Vec::new(),
            });
        }

        FrontendMessage::RequestLspRecommendations { language } => {
            debug!("headless: request_lsp_recommendations lang={:?}", language);
            let cwd = engine.cwd();
            let installed: Vec<String> = allthecodes_plugins::loader::load_installed_plugins()
                .into_iter()
                .map(|plugin| plugin.id)
                .collect();
            let mut recommendations = allthecodes_lsp_service::generate_recommendations(
                std::path::Path::new(&cwd),
                &installed,
            );
            if let Some(language) = language.as_ref() {
                recommendations.retain(|rec| {
                    rec.languages
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(language))
                });
            }
            let _ = sink.send(&BackendMessage::LspRecommendations {
                recommendations: recommendations
                    .into_iter()
                    .map(|rec| LspRecommendationDTO {
                        plugin_id: rec.plugin_id,
                        plugin_name: rec.plugin_name,
                        description: Some(rec.description),
                        languages: rec.languages,
                        confidence: rec.confidence,
                        is_installed: rec.is_already_installed,
                        is_dismissed: false,
                    })
                    .collect(),
            });
        }
    }

    true // continue loop
}

fn compute_headless_completions(input: &str, cursor_pos: usize) -> Vec<CompletionItemDTO> {
    let mut completer = CombinedCompleter::new();
    completer.add_provider(Box::new(CommandCompletionProvider::new()));
    completer.add_provider(Box::new(PathCompletionProvider::new()));
    completer.add_provider(Box::new(ShellHistoryCompletionProvider::new()));
    completer.add_provider(Box::new(SlackChannelCompletionProvider::new()));
    let ctx = CompletionContext::new(input, cursor_pos.min(input.len()), &[]);
    completer
        .compute(&ctx)
        .into_iter()
        .map(completion_item_to_dto)
        .collect()
}

fn completion_item_to_dto(item: CompletionItem) -> CompletionItemDTO {
    CompletionItemDTO {
        label: item.label,
        insert_text: item.insert_text,
        kind: match item.kind {
            CompletionKind::Command => "command",
            CompletionKind::Path => "path",
            CompletionKind::ShellHistory => "shell_history",
            CompletionKind::SlackChannel => "slack_channel",
        }
        .to_string(),
        detail: item.detail,
    }
}

#[cfg(feature = "telemetry")]
fn telemetry_bridge_active() -> bool {
    allthecodes_engine::telemetry_bridge::is_active()
}

#[cfg(not(feature = "telemetry"))]
fn telemetry_bridge_active() -> bool {
    false
}

// ---------------------------------------------------------------------------
// Slash command execution
// ---------------------------------------------------------------------------

fn conversation_changed(before: &[Message], after: &[Message]) -> bool {
    before.len() != after.len()
        || before
            .iter()
            .zip(after.iter())
            .any(|(lhs, rhs)| lhs.uuid() != rhs.uuid())
}

fn flatten_blocks(blocks: &[ContentBlock]) -> (String, Option<String>) {
    let mut text_parts = Vec::new();
    let mut thinking_parts = Vec::new();

    for block in blocks {
        match block {
            ContentBlock::Text { text } => text_parts.push(text.clone()),
            ContentBlock::ConnectorText { connector_text, .. } => {
                text_parts.push(connector_text.clone())
            }
            ContentBlock::Thinking { thinking, .. } => thinking_parts.push(thinking.clone()),
            _ => {}
        }
    }

    let text = text_parts.join("\n");
    let thinking = if thinking_parts.is_empty() {
        None
    } else {
        Some(thinking_parts.join("\n"))
    };

    (text, thinking)
}

fn to_conversation_message(message: &Message) -> Option<ConversationMessage> {
    match message {
        Message::User(user) => {
            let content = match &user.content {
                MessageContent::Text(text) => text.clone(),
                MessageContent::Blocks(blocks) => flatten_blocks(blocks).0,
            };

            Some(ConversationMessage {
                id: user.uuid.to_string(),
                role: "user".to_string(),
                content,
                timestamp: user.timestamp,
                content_blocks: match &user.content {
                    MessageContent::Blocks(blocks) => Some(blocks.clone()),
                    MessageContent::Text(_) => None,
                },
                cost_usd: None,
                thinking: None,
                level: None,
            })
        }
        Message::Assistant(assistant) => {
            let (content, thinking) = flatten_blocks(&assistant.content);
            Some(ConversationMessage {
                id: assistant.uuid.to_string(),
                role: "assistant".to_string(),
                content,
                timestamp: assistant.timestamp,
                content_blocks: Some(assistant.content.clone()),
                cost_usd: Some(assistant.cost_usd),
                thinking,
                level: None,
            })
        }
        Message::System(system) => {
            let level = match &system.subtype {
                allthecodes_types::message::SystemSubtype::Informational { level } => Some(
                    match level {
                        allthecodes_types::message::InfoLevel::Info => "info",
                        allthecodes_types::message::InfoLevel::Warning => "warning",
                        allthecodes_types::message::InfoLevel::Error => "error",
                    }
                    .to_string(),
                ),
                allthecodes_types::message::SystemSubtype::Warning => Some("warning".to_string()),
                allthecodes_types::message::SystemSubtype::ApiError { .. } => {
                    Some("error".to_string())
                }
                _ => Some("info".to_string()),
            };

            Some(ConversationMessage {
                id: system.uuid.to_string(),
                role: "system".to_string(),
                content: system.content.clone(),
                timestamp: system.timestamp,
                content_blocks: None,
                cost_usd: None,
                thinking: None,
                level,
            })
        }
        _ => None,
    }
}

fn send_conversation_replaced(messages: &[Message], sink: &FrontendSink) {
    let visible_messages: Vec<_> = messages
        .iter()
        .filter_map(to_conversation_message)
        .collect();
    let _ = sink.send(&BackendMessage::ConversationReplaced {
        messages: visible_messages,
    });
}

fn send_ready_snapshot(engine: &QueryEngine, sink: &FrontendSink) {
    let app_state = engine.app_state();
    let keybindings = app_state
        .keybindings
        .user_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());

    let _ = sink.send(&BackendMessage::Ready {
        session_id: engine.current_session_id().to_string(),
        model: app_state.main_loop_model.clone(),
        cwd: engine.cwd().to_string(),
        permission_mode: app_state.tool_permission_context.mode.as_str().to_string(),
        available_models: app_state.settings.available_models.clone(),
        plan_workflow: app_state.plan_workflow.clone(),
        editor_mode: app_state.settings.editor_mode.clone(),
        view_mode: app_state.settings.view_mode.clone(),
        keybindings,
    });
}

/// Parse and execute a slash command, sending results as BackendMessages.
async fn handle_slash_command(
    raw: &str,
    engine: &Arc<QueryEngine>,
    runtime: &SessionRuntime,
    suggestion_svc: &Arc<Mutex<PromptSuggestionService>>,
    sink: &FrontendSink,
) {
    let trimmed = raw.trim();
    if !trimmed.starts_with('/') {
        let _ = sink.send(&BackendMessage::Error {
            message: format!("not a slash command: {}", trimmed),
            recoverable: true,
        });
        return;
    }

    let cwd = std::path::PathBuf::from(engine.cwd());
    let dispatcher = allthecodes_commands::DefaultCommandDispatcher::for_cwd(&cwd);
    let Some(parsed) = dispatcher.parse_command_input(trimmed) else {
        let _ = sink.send(&BackendMessage::Error {
            message: format!("unknown command: {}", trimmed),
            recoverable: true,
        });
        return;
    };
    let command_name = dispatcher
        .command_name(parsed.index)
        .unwrap_or_else(|| trimmed.trim_start_matches('/').to_string());

    let original_messages = engine.messages();
    let original_app_state = engine.app_state();
    let original_plan = original_app_state.plan_workflow.clone();
    let original_mode = original_app_state.tool_permission_context.mode.clone();

    let mut ctx = CommandContext {
        messages: original_messages.clone(),
        cwd,
        app_state: original_app_state,
        session_id: engine.current_session_id(),
        hook_runner: engine.hook_runner(),
    };

    let command_executor = engine.command_executor();
    let cmd_result = command_executor
        .execute(parsed, command_name.clone(), &mut ctx)
        .await;

    // Sync any state mutations (e.g. /add-dir, /team) back to the engine.
    // Commands operate on a cloned snapshot; without this sync their edits
    // would be lost to the next tool invocation.
    if cmd_result.is_ok() {
        let plan_changed = ctx.app_state.plan_workflow != original_plan
            || ctx.app_state.tool_permission_context.mode != original_mode;
        crate::plan_workflow::sync_command_app_state(engine, &ctx.app_state);

        if plan_changed {
            if let Some(record) = ctx.app_state.plan_workflow.clone() {
                let _ = sink.send(&BackendMessage::PlanWorkflowEvent {
                    event: "slash_command".to_string(),
                    summary: allthecodes_types::plan_workflow::summarize(&record),
                    record,
                });
            }
        }

        // If this was a /team command (or any command that mutated team_context),
        // push a fresh StatusSnapshot so the Team Dashboard reflects the change
        // without the frontend having to poll.
        if command_name == "team" {
            if let Some(tc) = ctx.app_state.team_context.as_ref() {
                if !tc.team_name.is_empty() {
                    let events =
                        allthecodes_ipc::agent_handlers::build_team_status_events(&tc.team_name);
                    let _ = sink.send_many(events);
                }
            }
        }
    }

    match cmd_result {
        Ok(result) => match result {
            CommandResult::Output(text) => {
                if conversation_changed(&original_messages, &ctx.messages) {
                    engine.replace_messages(ctx.messages.clone());
                    send_conversation_replaced(&ctx.messages, sink);
                }
                let _ = sink.send(&BackendMessage::SystemInfo {
                    text,
                    level: "info".to_string(),
                });
            }
            CommandResult::SwitchSession {
                session_id,
                messages,
                notice,
            } => {
                if runtime.set_session_id(session_id.to_string()).is_err() {
                    let _ = sink.send(&BackendMessage::Error {
                        message: "Cannot switch sessions while a query turn is running."
                            .to_string(),
                        recoverable: true,
                    });
                    return;
                }
                engine.set_current_session_id(session_id);
                engine.replace_messages(messages.clone());
                send_conversation_replaced(&messages, sink);
                send_ready_snapshot(engine, sink);
                let _ = sink.send(&BackendMessage::SystemInfo {
                    text: notice,
                    level: "info".to_string(),
                });
            }
            CommandResult::Clear => {
                engine.start_new_session();
                let _ = runtime.set_session_id(engine.current_session_id().to_string());
                send_conversation_replaced(&[], sink);
                send_ready_snapshot(engine, sink);
                let _ = sink.send(&BackendMessage::SystemInfo {
                    text: "Started a new session.".to_string(),
                    level: "info".to_string(),
                });
            }
            CommandResult::Exit(msg) => {
                let _ = sink.send(&BackendMessage::SystemInfo {
                    text: msg,
                    level: "info".to_string(),
                });
            }
            CommandResult::Query(msgs) => {
                if conversation_changed(&original_messages, &ctx.messages) {
                    engine.replace_messages(ctx.messages.clone());
                    send_conversation_replaced(&ctx.messages, sink);
                }
                let prompt_text: String = msgs
                    .iter()
                    .filter_map(|m| match m {
                        Message::User(u) => Some(match &u.content {
                            allthecodes_types::message::MessageContent::Text(t) => t.clone(),
                            allthecodes_types::message::MessageContent::Blocks(_) => {
                                "[content blocks]".to_string()
                            }
                        }),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                if !prompt_text.is_empty()
                    && !allthecodes_ipc::client::query_runner::spawn_managed_query_turn(
                        runtime.clone(),
                        engine.clone(),
                        prompt_text,
                        uuid::Uuid::new_v4().to_string(),
                        suggestion_svc.clone(),
                        sink.clone(),
                        handle_sdk_message,
                    )
                {
                    let _ = sink.send(&BackendMessage::Error {
                        message: "A query turn is already running; command query was not started."
                            .to_string(),
                        recoverable: true,
                    });
                }
            }
            CommandResult::None => {
                if conversation_changed(&original_messages, &ctx.messages) {
                    engine.replace_messages(ctx.messages.clone());
                    send_conversation_replaced(&ctx.messages, sink);
                }
            }
        },
        Err(e) => {
            let _ = sink.send(&BackendMessage::Error {
                message: format!("command error: {}", e),
                recoverable: true,
            });
        }
    }
}
