use allthecodes_types::message::Message;

use crate::ui::messages::render::{
    build_message_render_context_with_options, MessageRenderContext, MessageRenderOptions,
};

#[derive(Debug, Clone)]
pub(crate) struct MessageListViewModel {
    source_len: usize,
    render_context: MessageRenderContext,
}

impl MessageListViewModel {
    pub(crate) fn build(
        messages: &[Message],
        selected_message: Option<usize>,
        selected_expanded: bool,
        options: MessageRenderOptions,
    ) -> Self {
        Self {
            source_len: messages.len(),
            render_context: build_message_render_context_with_options(
                messages,
                selected_message,
                selected_expanded,
                options,
            ),
        }
    }

    pub(crate) fn source_len(&self) -> usize {
        self.source_len
    }

    pub(crate) fn render_context(&self) -> &MessageRenderContext {
        &self.render_context
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{Message, MessageContent, UserMessage};

    fn user_message(text: &str) -> Message {
        Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1,
            role: "user".to_string(),
            content: MessageContent::Text(text.to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    #[test]
    fn message_view_model_preserves_selection_inputs() {
        let messages = vec![user_message("hello")];
        let vm =
            MessageListViewModel::build(&messages, Some(0), true, MessageRenderOptions::default());

        assert_eq!(vm.source_len(), 1);
        assert_eq!(vm.render_context().selected_message(), Some(0));
        assert!(vm.render_context().selected_expanded());
    }
}
