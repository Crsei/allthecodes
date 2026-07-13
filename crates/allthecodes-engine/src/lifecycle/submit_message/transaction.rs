use std::sync::Arc;

use allthecodes_types::sdk::{SdkMessage, SdkResult};
use uuid::Uuid;

use crate::bootstrap::SessionId;
use crate::session::record_replay::types::RecordItem;
use crate::session::transcript;
use crate::types::config::QueryEngineConfig;
use crate::types::message::{Message, Usage};

use super::super::QueryEngineState;

#[derive(Debug, Clone, Default)]
pub(crate) struct PersistencePlan {
    pub(crate) transcript_messages: Vec<Message>,
    pub(crate) save_session_after_commit: bool,
    pub(crate) record_items: Vec<RecordItem>,
    pub(crate) record_context: Option<&'static str>,
    pub(crate) flush_recorder_after_commit: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SubmitTransaction {
    pub(crate) appended_messages: Vec<Message>,
    pub(crate) removed_assistant_messages: Vec<Uuid>,
    pub(crate) turn_count_increment: usize,
    pub(crate) usage_delta: Option<Usage>,
    pub(crate) cost_delta_usd: f64,
    pub(crate) emitted_events: Vec<SdkMessage>,
    pub(crate) terminal_result: Option<SdkResult>,
    pub(crate) persistence: PersistencePlan,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SubmitTransactionOutcome {
    pub(crate) emitted_events: Vec<SdkMessage>,
    pub(crate) terminal_result: Option<SdkResult>,
    pub(crate) record_items: Vec<RecordItem>,
    pub(crate) record_context: Option<&'static str>,
    pub(crate) flush_recorder: bool,
}

impl SubmitTransactionOutcome {
    pub(crate) fn into_sdk_messages(self) -> Vec<SdkMessage> {
        let mut messages = self.emitted_events;
        if let Some(result) = self.terminal_result {
            messages.push(SdkMessage::Result(result));
        }
        messages
    }
}

impl SubmitTransaction {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn append_message(&mut self, message: Message) {
        self.appended_messages.push(message);
    }

    pub(crate) fn remove_assistant_message(&mut self, uuid: Uuid) {
        self.removed_assistant_messages.push(uuid);
    }

    pub(crate) fn increment_turn_count(&mut self) {
        self.turn_count_increment = self.turn_count_increment.saturating_add(1);
    }

    pub(crate) fn record_usage_cost(&mut self, usage: Usage, cost_usd: f64) {
        match self.usage_delta.as_mut() {
            Some(delta) => merge_usage(delta, &usage),
            None => self.usage_delta = Some(usage),
        }
        self.cost_delta_usd += cost_usd;
    }

    pub(crate) fn persist(&mut self, message: Message) {
        self.persistence.transcript_messages.push(message);
    }

    pub(crate) fn save_session_after_commit(&mut self) {
        self.persistence.save_session_after_commit = true;
    }

    pub(crate) fn record_items(&mut self, context: &'static str, items: Vec<RecordItem>) {
        if items.is_empty() {
            return;
        }
        self.persistence.record_context = Some(context);
        self.persistence.record_items.extend(items);
    }

    pub(crate) fn flush_recorder_after_commit(&mut self) {
        self.persistence.flush_recorder_after_commit = true;
    }

    pub(crate) fn emit(&mut self, event: SdkMessage) {
        self.emitted_events.push(event);
    }

    pub(crate) fn terminate(&mut self, result: SdkResult) {
        self.terminal_result = Some(result);
    }

    pub(crate) fn commit(
        self,
        state_ref: &Arc<parking_lot::RwLock<QueryEngineState>>,
        session_id: &SessionId,
        config: &QueryEngineConfig,
    ) -> SubmitTransactionOutcome {
        let persistence = self.persistence;
        {
            let mut state = state_ref.write();
            if !self.removed_assistant_messages.is_empty() {
                state.transcript.messages.retain(|message| {
                    !matches!(
                        message,
                        Message::Assistant(assistant)
                            if self.removed_assistant_messages.contains(&assistant.uuid)
                    )
                });
            }
            for message in self.appended_messages {
                state.append_message(message);
            }
            state.transcript.total_turn_count = state
                .transcript
                .total_turn_count
                .saturating_add(self.turn_count_increment);
            if let Some(usage) = self.usage_delta {
                state.update_usage(&usage, self.cost_delta_usd);
            }
        }

        if !persistence.transcript_messages.is_empty() {
            let _ = transcript::record_transcript(
                session_id.as_str(),
                &persistence.transcript_messages,
            );
        }

        if persistence.save_session_after_commit && config.auto_save_session {
            let all_msgs = state_ref.read().transcript.messages.clone();
            let _ =
                crate::session::storage::save_session(session_id.as_str(), &all_msgs, &config.cwd);
        }

        SubmitTransactionOutcome {
            emitted_events: self.emitted_events,
            terminal_result: self.terminal_result,
            record_items: persistence.record_items,
            record_context: persistence.record_context,
            flush_recorder: persistence.flush_recorder_after_commit,
        }
    }
}

fn merge_usage(target: &mut Usage, source: &Usage) {
    target.input_tokens = target.input_tokens.saturating_add(source.input_tokens);
    target.output_tokens = target.output_tokens.saturating_add(source.output_tokens);
    target.cache_read_input_tokens = target
        .cache_read_input_tokens
        .saturating_add(source.cache_read_input_tokens);
    target.cache_creation_input_tokens = target
        .cache_creation_input_tokens
        .saturating_add(source.cache_creation_input_tokens);
    target.reasoning_output_tokens = target
        .reasoning_output_tokens
        .saturating_add(source.reasoning_output_tokens);
}

#[cfg(test)]
mod submit_transaction_tests {
    use super::*;
    use allthecodes_types::sdk::{ResultSubtype, SdkAssistantMessage, SdkMessage, SdkResult};

    use crate::lifecycle::QueryEngine;
    use crate::session::record_replay::types::{RecordItem, TurnFinishStatus, TurnFinishedRecord};
    use crate::types::config::QueryEngineConfig;
    use crate::types::message::{AssistantMessage, ContentBlock, Message};

    fn make_config() -> QueryEngineConfig {
        QueryEngineConfig {
            cwd: "/tmp".to_string(),
            tools: vec![],
            custom_system_prompt: None,
            append_system_prompt: None,
            user_specified_model: None,
            fallback_model: None,
            max_turns: None,
            max_budget_usd: None,
            task_budget: None,
            verification_policy: None,
            verbose: false,
            initial_messages: None,
            commands: vec![],
            thinking_config: None,
            json_schema: None,
            replay_user_messages: false,
            persist_session: false,
            resolved_model: None,
            auto_save_session: false,
            agent_context: None,
        }
    }

    fn assistant_message() -> AssistantMessage {
        AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1,
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: "done".to_string(),
            }],
            usage: Some(Usage {
                input_tokens: 3,
                output_tokens: 5,
                cache_read_input_tokens: 7,
                cache_creation_input_tokens: 11,
                reasoning_output_tokens: 13,
            }),
            stop_reason: Some("end_turn".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.25,
        }
    }

    #[test]
    fn submit_transaction_accumulates_append_persistence_usage_and_events() {
        let assistant = assistant_message();
        let sdk_message = SdkMessage::Assistant(SdkAssistantMessage {
            message: assistant.clone(),
            session_id: "session-1".to_string(),
            parent_tool_use_id: None,
        });
        let mut transaction = SubmitTransaction::new();

        transaction.append_message(Message::Assistant(assistant.clone()));
        transaction.record_usage_cost(assistant.usage.as_ref().unwrap().clone(), 0.0);
        transaction.persist(Message::Assistant(assistant.clone()));
        transaction.emit(sdk_message);

        assert_eq!(transaction.appended_messages.len(), 1);
        assert_eq!(transaction.persistence.transcript_messages.len(), 1);
        assert_eq!(
            transaction
                .usage_delta
                .as_ref()
                .map(|usage| usage.input_tokens),
            Some(3)
        );
        assert_eq!(transaction.emitted_events.len(), 1);
    }

    #[test]
    fn submit_transaction_merges_usage_deltas() {
        let mut transaction = SubmitTransaction::new();

        transaction.record_usage_cost(
            Usage {
                input_tokens: 1,
                output_tokens: 2,
                cache_read_input_tokens: 3,
                cache_creation_input_tokens: 4,
                reasoning_output_tokens: 5,
            },
            0.0,
        );
        transaction.record_usage_cost(
            Usage {
                input_tokens: 10,
                output_tokens: 20,
                cache_read_input_tokens: 30,
                cache_creation_input_tokens: 40,
                reasoning_output_tokens: 50,
            },
            0.0,
        );

        let usage = transaction.usage_delta.expect("usage delta");
        assert_eq!(usage.input_tokens, 11);
        assert_eq!(usage.output_tokens, 22);
        assert_eq!(usage.cache_read_input_tokens, 33);
        assert_eq!(usage.cache_creation_input_tokens, 44);
        assert_eq!(usage.reasoning_output_tokens, 55);
    }

    #[test]
    fn submit_transaction_commit_applies_state_and_returns_events() {
        let engine = QueryEngine::new(make_config());
        let assistant = assistant_message();
        let sdk_message = SdkMessage::Assistant(SdkAssistantMessage {
            message: assistant.clone(),
            session_id: engine.session_id.to_string(),
            parent_tool_use_id: None,
        });
        let mut transaction = SubmitTransaction::new();
        transaction.append_message(Message::Assistant(assistant.clone()));
        transaction.record_usage_cost(
            assistant.usage.as_ref().unwrap().clone(),
            assistant.cost_usd,
        );
        transaction.emit(sdk_message);

        let outcome = transaction.commit(&engine.state, &engine.session_id, &engine.config);

        assert_eq!(outcome.emitted_events.len(), 1);
        let state = engine.state.read();
        assert_eq!(state.transcript.messages.len(), 1);
        assert_eq!(state.transcript.usage.total_input_tokens, 3);
        assert_eq!(state.transcript.usage.total_output_tokens, 5);
        assert_eq!(state.transcript.usage.total_cost_usd, 0.25);
    }

    #[test]
    fn submit_transaction_removes_tombstoned_message_and_requests_session_save() {
        let engine = QueryEngine::new(make_config());
        let assistant = assistant_message();
        engine
            .state
            .write()
            .append_message(Message::Assistant(assistant.clone()));
        let mut transaction = SubmitTransaction::new();

        transaction.remove_assistant_message(assistant.uuid);
        transaction.save_session_after_commit();

        let outcome = transaction.commit(&engine.state, &engine.session_id, &engine.config);

        assert!(outcome.emitted_events.is_empty());
        assert!(engine.state.read().transcript.messages.is_empty());
    }

    #[test]
    fn submit_transaction_carries_terminal_result_record_items_and_flush_plan() {
        let engine = QueryEngine::new(make_config());
        let result = SdkResult {
            subtype: ResultSubtype::Success,
            is_error: false,
            duration_ms: 1,
            duration_api_ms: 1,
            num_turns: 0,
            result: "done".to_string(),
            stop_reason: None,
            session_id: engine.session_id.to_string(),
            total_cost_usd: 0.0,
            usage: Default::default(),
            permission_denials: vec![],
            structured_output: None,
            uuid: uuid::Uuid::new_v4(),
            errors: vec![],
        };
        let mut transaction = SubmitTransaction::new();
        transaction.record_items(
            "turn_finished",
            vec![RecordItem::TurnFinished(TurnFinishedRecord {
                status: TurnFinishStatus::Completed,
                abort_reason: None,
                error: None,
                usage: None,
                review_proposal_ids: Vec::new(),
            })],
        );
        transaction.flush_recorder_after_commit();
        transaction.terminate(result);

        let outcome = transaction.commit(&engine.state, &engine.session_id, &engine.config);

        assert_eq!(outcome.record_items.len(), 1);
        assert_eq!(outcome.record_context, Some("turn_finished"));
        assert!(outcome.flush_recorder);
        assert!(matches!(
            outcome.into_sdk_messages().last(),
            Some(SdkMessage::Result(result)) if result.result == "done"
        ));
    }
}
