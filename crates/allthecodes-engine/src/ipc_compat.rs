use std::sync::Arc;

use crate::lifecycle::QueryEngine;

impl allthecodes_types::callbacks::CallbackHost for QueryEngine {
    fn set_permission_callback(&self, cb: crate::types::tool::PermissionCallback) {
        QueryEngine::set_permission_callback(self, cb);
    }

    fn set_ask_user_callback(&self, cb: crate::types::tool::AskUserCallback) {
        QueryEngine::set_ask_user_callback(self, cb);
    }

    fn set_permission_event_callback(&self, cb: crate::types::tool::PermissionEventCallback) {
        QueryEngine::set_permission_event_callback(self, cb);
    }

    fn set_tool_progress_callback(
        &self,
        cb: Arc<dyn Fn(crate::types::tool::ToolProgress) + Send + Sync>,
    ) {
        QueryEngine::set_tool_progress_callback(self, cb);
    }
}

impl allthecodes_types::query_host::QueryTurnHost<allthecodes_types::sdk::SdkMessage>
    for QueryEngine
{
    type Stream = futures::stream::BoxStream<'static, allthecodes_types::sdk::SdkMessage>;

    fn reset_abort(&self) {
        QueryEngine::reset_abort(self);
    }

    fn submit_client_message(&self, prompt_text: &str) -> Self::Stream {
        Box::pin(QueryEngine::submit_message(
            self,
            prompt_text,
            crate::types::config::QuerySource::ReplMainThread,
        ))
    }
}
