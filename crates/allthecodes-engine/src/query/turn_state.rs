#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryTurnPhase {
    Preparing,
    Streaming,
    ToolExecution,
    Finished,
    Aborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryTurnAbortPoint {
    Preparing,
    Streaming,
    ToolExecution,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QueryTurnTransitionError {
    pub(crate) from: QueryTurnPhase,
    pub(crate) attempted: &'static str,
}

impl QueryTurnTransitionError {
    pub(crate) fn to_terminal_message(
        self,
        turn: usize,
    ) -> crate::types::message::AssistantMessage {
        crate::types::message::AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![crate::types::message::ContentBlock::Text {
                text: format!(
                    "Query turn lifecycle transition failed on turn {turn}: attempted {} from {:?}.",
                    self.attempted, self.from
                ),
            }],
            usage: None,
            stop_reason: Some("query_turn_transition_error".to_string()),
            is_api_error_message: true,
            api_error: None,
            cost_usd: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueryTurnState {
    turn: usize,
    phase: QueryTurnPhase,
    abort_point: Option<QueryTurnAbortPoint>,
}

impl QueryTurnState {
    pub(crate) fn new(turn: usize) -> Self {
        Self {
            turn,
            phase: QueryTurnPhase::Preparing,
            abort_point: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn turn(&self) -> usize {
        self.turn
    }

    #[cfg(test)]
    pub(crate) fn phase(&self) -> QueryTurnPhase {
        self.phase
    }

    #[cfg(test)]
    pub(crate) fn abort_point(&self) -> Option<QueryTurnAbortPoint> {
        self.abort_point
    }

    pub(crate) fn start_streaming(&mut self) -> Result<(), QueryTurnTransitionError> {
        if self.phase == QueryTurnPhase::Streaming {
            return Ok(());
        }
        self.transition(
            QueryTurnPhase::Preparing,
            QueryTurnPhase::Streaming,
            "start_streaming",
        )
    }

    pub(crate) fn finish_streaming(
        &mut self,
        has_tool_calls: bool,
    ) -> Result<(), QueryTurnTransitionError> {
        let next = if has_tool_calls {
            QueryTurnPhase::ToolExecution
        } else {
            QueryTurnPhase::Finished
        };
        self.transition(QueryTurnPhase::Streaming, next, "finish_streaming")
    }

    pub(crate) fn finish_tool_execution(&mut self) -> Result<(), QueryTurnTransitionError> {
        self.transition(
            QueryTurnPhase::ToolExecution,
            QueryTurnPhase::Finished,
            "finish_tool_execution",
        )
    }

    pub(crate) fn abort(&mut self) {
        self.abort_point = Some(match self.phase {
            QueryTurnPhase::Preparing => QueryTurnAbortPoint::Preparing,
            QueryTurnPhase::Streaming => QueryTurnAbortPoint::Streaming,
            QueryTurnPhase::ToolExecution => QueryTurnAbortPoint::ToolExecution,
            QueryTurnPhase::Finished | QueryTurnPhase::Aborted => QueryTurnAbortPoint::Finished,
        });
        self.phase = QueryTurnPhase::Aborted;
    }

    fn transition(
        &mut self,
        expected: QueryTurnPhase,
        next: QueryTurnPhase,
        attempted: &'static str,
    ) -> Result<(), QueryTurnTransitionError> {
        if self.phase != expected {
            return Err(QueryTurnTransitionError {
                from: self.phase,
                attempted,
            });
        }
        self.phase = next;
        Ok(())
    }
}

#[cfg(test)]
mod query_turn_state_tests {
    use super::*;

    #[test]
    fn query_turn_transitions_from_preparing_to_finished() {
        let mut state = QueryTurnState::new(3);

        assert_eq!(state.turn(), 3);
        assert_eq!(state.phase(), QueryTurnPhase::Preparing);

        state.start_streaming().expect("start streaming");
        state.finish_streaming(false).expect("finish streaming");

        assert_eq!(state.phase(), QueryTurnPhase::Finished);
        assert_eq!(state.abort_point(), None);
    }

    #[test]
    fn query_turn_rejects_streaming_after_tool_execution() {
        let mut state = QueryTurnState::new(1);

        state.start_streaming().expect("start streaming");
        state.finish_streaming(true).expect("enter tool execution");

        let error = state
            .start_streaming()
            .expect_err("streaming cannot restart after tool execution begins");

        assert_eq!(state.phase(), QueryTurnPhase::ToolExecution);
        assert_eq!(
            error,
            QueryTurnTransitionError {
                from: QueryTurnPhase::ToolExecution,
                attempted: "start_streaming",
            }
        );
    }

    #[test]
    fn query_turn_records_abort_during_tool_execution() {
        let mut state = QueryTurnState::new(2);

        state.start_streaming().expect("start streaming");
        state.finish_streaming(true).expect("enter tool execution");
        state.abort();

        assert_eq!(state.phase(), QueryTurnPhase::Aborted);
        assert_eq!(
            state.abort_point(),
            Some(QueryTurnAbortPoint::ToolExecution)
        );
    }

    #[test]
    fn query_turn_transition_error_becomes_terminal_event_message() {
        let mut state = QueryTurnState::new(7);
        state.start_streaming().expect("start streaming");
        state.finish_streaming(true).expect("enter tool execution");
        let error = state
            .start_streaming()
            .expect_err("invalid restart should be rejected");

        let message = error.to_terminal_message(7);

        assert!(message.is_api_error_message);
        assert_eq!(
            message.stop_reason.as_deref(),
            Some("query_turn_transition_error")
        );
        assert!(message.content.iter().any(
            |block| matches!(block, crate::types::message::ContentBlock::Text { text }
                if text.contains("start_streaming") && text.contains("ToolExecution"))
        ));
    }
}
