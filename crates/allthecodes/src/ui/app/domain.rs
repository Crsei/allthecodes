use std::collections::VecDeque;

use allthecodes_types::message::Message;
use ratatui::layout::Rect;

use crate::ui::history_search_dialog::HistorySearchEntry;
use crate::ui::messages::{MessageRenderContext, MessageRenderOptions};
use crate::ui::theme::Theme;
use crate::ui::virtual_scroll::VirtualScroll;

pub(super) struct ConversationStore {
    messages: Vec<Message>,
    selected_message: Option<usize>,
    selected_message_expanded: bool,
    scroll_offset: usize,
    vscroll: VirtualScroll,
}

impl Default for ConversationStore {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            selected_message: None,
            selected_message_expanded: false,
            scroll_offset: 0,
            vscroll: VirtualScroll::new(),
        }
    }
}

impl std::fmt::Debug for ConversationStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConversationStore")
            .field("messages", &self.messages)
            .field("selected_message", &self.selected_message)
            .field("selected_message_expanded", &self.selected_message_expanded)
            .field("scroll_offset", &self.scroll_offset)
            .finish_non_exhaustive()
    }
}

impl ConversationStore {
    pub(super) fn add_message(&mut self, msg: Message) {
        self.messages.push(msg);
        self.clamp_selection();
        self.vscroll
            .invalidate_from(self.messages.len().saturating_sub(1));
    }

    pub(super) fn replace_last_message(&mut self, msg: Message) {
        if let Some(last) = self.messages.last_mut() {
            *last = msg;
        } else {
            self.messages.push(msg);
        }
        self.clamp_selection();
        self.vscroll
            .invalidate_from(self.messages.len().saturating_sub(1));
    }

    pub(super) fn remove_last_message(&mut self) -> bool {
        let removed = self.messages.pop().is_some();
        if removed {
            self.clamp_selection();
            self.vscroll.invalidate_from(self.messages.len());
        }
        removed
    }

    pub(super) fn clear(&mut self) {
        self.messages.clear();
        self.selected_message = None;
        self.selected_message_expanded = false;
        self.scroll_offset = 0;
        self.vscroll.invalidate_all();
    }

    pub(super) fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub(super) fn selection(&self) -> Option<usize> {
        self.selected_message
    }

    pub(super) fn set_selection(&mut self, selected: Option<usize>) {
        self.selected_message = selected;
        self.clamp_selection();
    }

    pub(super) fn selected_expanded(&self) -> bool {
        self.selected_message_expanded
    }

    pub(super) fn set_selected_expanded(&mut self, expanded: bool) {
        self.selected_message_expanded = expanded;
    }

    pub(super) fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub(super) fn set_scroll_offset(&mut self, offset: usize) {
        self.scroll_offset = offset;
    }

    pub(super) fn vscroll(&self) -> &VirtualScroll {
        &self.vscroll
    }

    pub(super) fn ensure_vscroll_up_to_date(
        &mut self,
        width: u16,
        theme: &Theme,
        render_context: &MessageRenderContext,
    ) {
        self.vscroll
            .ensure_up_to_date(&self.messages, width, theme, render_context);
    }

    pub(super) fn render_context_inputs(&self) -> (Option<usize>, bool) {
        (self.selected_message, self.selected_message_expanded)
    }

    fn clamp_selection(&mut self) {
        if self.messages.is_empty() {
            self.selected_message = None;
            self.selected_message_expanded = false;
            return;
        }
        if let Some(index) = self.selected_message {
            self.selected_message = Some(index.min(self.messages.len() - 1));
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct PromptQueueStore {
    queued_prompts: VecDeque<String>,
}

impl PromptQueueStore {
    pub(super) fn queue(&mut self, text: String) -> usize {
        self.queued_prompts.push_back(text);
        self.queued_prompts.len()
    }

    pub(super) fn pop_next(&mut self) -> Option<String> {
        self.queued_prompts.pop_front()
    }

    pub(super) fn len(&self) -> usize {
        self.queued_prompts.len()
    }
}

#[derive(Debug, Default)]
pub(super) struct SessionUiStore {
    pub(super) model_name: String,
    pub(super) backend_name: String,
    pub(super) session_id: String,
    pub(super) cwd: String,
    pub(super) output_style: Option<String>,
    pub(super) history: Vec<HistorySearchEntry>,
}

#[derive(Debug, Default)]
pub(super) struct RenderLayoutStore {
    pub(super) session_scrollbar: Option<super::SessionScrollbarState>,
    pub(super) session_scrollbar_dragging: bool,
    pub(super) message_area: Option<Rect>,
    pub(super) prompt_area: Option<Rect>,
}

pub(super) fn build_message_render_context(
    conversation: &ConversationStore,
    options: MessageRenderOptions,
) -> MessageRenderContext {
    let (selected_message, selected_expanded) = conversation.render_context_inputs();
    crate::ui::messages::build_message_render_context_with_options(
        conversation.messages(),
        selected_message,
        selected_expanded,
        options,
    )
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
    fn conversation_store_clamps_selection_after_remove() {
        let mut store = ConversationStore::default();
        store.add_message(user_message("one"));
        store.add_message(user_message("two"));
        store.set_selection(Some(1));

        store.remove_last_message();

        assert_eq!(store.messages().len(), 1);
        assert_eq!(store.selection(), Some(0));
    }

    #[test]
    fn prompt_queue_store_preserves_fifo_order() {
        let mut store = PromptQueueStore::default();

        assert_eq!(store.queue("first".to_string()), 1);
        assert_eq!(store.queue("second".to_string()), 2);

        assert_eq!(store.pop_next().as_deref(), Some("first"));
        assert_eq!(store.pop_next().as_deref(), Some("second"));
        assert_eq!(store.pop_next(), None);
    }
}
