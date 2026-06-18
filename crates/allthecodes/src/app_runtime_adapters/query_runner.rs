//! Compatibility adapter for the IPC client query-turn path.

use std::sync::Arc;

use parking_lot::Mutex;

use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_ipc::transport::FrontendSink;
use allthecodes_services::prompt_suggestion::PromptSuggestionService;

use super::sdk_mapper::handle_sdk_message;

/// Spawn a query turn as a background tokio task.
pub fn spawn_query_turn(
    engine: Arc<QueryEngine>,
    prompt_text: String,
    message_id: String,
    suggestion_svc: Arc<Mutex<PromptSuggestionService>>,
    sink: FrontendSink,
) {
    allthecodes_ipc::client::query_runner::spawn_query_turn(
        engine,
        prompt_text,
        message_id,
        suggestion_svc,
        sink,
        handle_sdk_message,
    );
}
