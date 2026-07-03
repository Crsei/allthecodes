use std::sync::Arc;

use allthecodes_types::sdk::SdkMessage;

use crate::bootstrap::SessionId;
use crate::session::transcript;
use crate::types::config::QueryEngineConfig;
use crate::types::message::{Message, Usage};

use super::super::QueryEngineState;

#[derive(Debug, Clone, Default)]
pub(crate) struct PersistencePlan {
    pub(crate) transcript_messages: Vec<Message>,
    pub(crate) save_session_after_commit: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SubmitTransaction {
    pub(crate) appended_messages: Vec<Message>,
    pub(crate) usage_delta: Option<Usage>,
    pub(crate) cost_delta_usd: f64,
    pub(crate) emitted_events: Vec<SdkMessage>,
    pub(crate) persistence: PersistencePlan,
}

impl SubmitTransaction {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn append_message(&mut self, message: Message) {
        self.appended_messages.push(message);
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

    pub(crate) fn emit(&mut self, event: SdkMessage) {
        self.emitted_events.push(event);
    }

    pub(crate) fn commit(
        self,
        state_ref: &Arc<parking_lot::RwLock<QueryEngineState>>,
        session_id: &SessionId,
        config: &QueryEngineConfig,
    ) -> Vec<SdkMessage> {
        {
            let mut state = state_ref.write();
            for message in self.appended_messages {
                state.append_message(message);
            }
            if let Some(usage) = self.usage_delta {
                state.update_usage(&usage, self.cost_delta_usd);
            }
        }

        if !self.persistence.transcript_messages.is_empty() {
            let _ = transcript::record_transcript(
                session_id.as_str(),
                &self.persistence.transcript_messages,
            );
        }

        if self.persistence.save_session_after_commit && config.auto_save_session {
            let all_msgs = state_ref.read().transcript.messages.clone();
            let _ =
                crate::session::storage::save_session(session_id.as_str(), &all_msgs, &config.cwd);
        }

        self.emitted_events
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
    use allthecodes_types::sdk::{SdkAssistantMessage, SdkMessage};

    use crate::lifecycle::QueryEngine;
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

        let events = transaction.commit(&engine.state, &engine.session_id, &engine.config);

        assert_eq!(events.len(), 1);
        let state = engine.state.read();
        assert_eq!(state.transcript.messages.len(), 1);
        assert_eq!(state.transcript.usage.total_input_tokens, 3);
        assert_eq!(state.transcript.usage.total_output_tokens, 5);
        assert_eq!(state.transcript.usage.total_cost_usd, 0.25);
    }
}
