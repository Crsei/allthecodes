//! Virtual scroll — only render messages visible in the viewport.
//!
//! Maintains a per-message height cache and a cumulative prefix-sum offset
//! array. Uses binary search to find the visible message range in O(log n)
//! instead of iterating all messages every frame.

use ratatui::text::Line;
use unicode_width::UnicodeWidthStr;

use allthecodes_types::message::Message;

use super::messages::{render_renderable_message_for_layout, MessageRenderContext};
use super::theme::Theme;

/// Number of extra lines to render above/below the viewport for smooth
/// scrolling.
const OVERSCAN: usize = 40;

pub struct VirtualScroll {
    /// Per-message logical line count (including +1 separator for all but last).
    /// This preserves the current prompt/transcript renderer contract.
    heights: Vec<usize>,
    /// Prefix-sum offsets: `offsets[i]` = sum of `heights[0..i]`.
    /// `offsets[0]` = 0, `offsets[n]` = total lines.
    offsets: Vec<usize>,
    /// Per-message wrapped visual line count at `cached_width`.
    visual_heights: Vec<usize>,
    /// Prefix-sum offsets for width-aware wrapped rendering.
    visual_offsets: Vec<usize>,
    /// Index from which the cache is invalid.
    valid_up_to: usize,
    /// Terminal width used for the cached heights.
    cached_width: u16,
    /// Message render-context cache key used for height calculations.
    cached_render_key: String,
}

impl VirtualScroll {
    pub fn new() -> Self {
        Self {
            heights: Vec::new(),
            offsets: vec![0],
            visual_heights: Vec::new(),
            visual_offsets: vec![0],
            valid_up_to: 0,
            cached_width: 0,
            cached_render_key: String::new(),
        }
    }

    /// Invalidate all cached heights (e.g. after `clear_messages`).
    pub fn invalidate_all(&mut self) {
        self.heights.clear();
        self.offsets.clear();
        self.offsets.push(0);
        self.visual_heights.clear();
        self.visual_offsets.clear();
        self.visual_offsets.push(0);
        self.valid_up_to = 0;
    }

    /// Invalidate from a specific message index onward.
    pub fn invalidate_from(&mut self, index: usize) {
        if index < self.valid_up_to {
            self.valid_up_to = index;
        }
        self.heights.truncate(index);
        self.offsets.truncate(index + 1); // keep offsets[0..=index]
        self.visual_heights.truncate(index);
        self.visual_offsets.truncate(index + 1);
    }

    /// Ensure heights and offsets are up-to-date for all messages.
    /// Re-computes only the invalidated tail.
    pub fn ensure_up_to_date(
        &mut self,
        _messages: &[Message],
        width: u16,
        theme: &Theme,
        render_context: &MessageRenderContext,
    ) {
        // Width changed → full invalidation
        if width != self.cached_width || render_context.cache_key() != self.cached_render_key {
            self.invalidate_all();
            self.cached_width = width;
            self.cached_render_key = render_context.cache_key().to_string();
        }

        let renderable_messages = render_context.renderable_messages();

        // Shrink if messages were removed
        if self.heights.len() > renderable_messages.len() {
            self.invalidate_from(renderable_messages.len());
        }

        let start = self.valid_up_to;
        let total = renderable_messages.len();

        for (i, message) in renderable_messages
            .iter()
            .enumerate()
            .take(total)
            .skip(start)
        {
            let lines = render_renderable_message_for_layout(
                message,
                i,
                theme,
                width as usize,
                render_context,
            );
            let mut h = lines.len();
            let mut visual_h = wrapped_line_height(&lines, width);
            // Separator blank line between messages (not after last)
            if i < total - 1 {
                h += 1;
                visual_h += 1;
            }
            if i < self.heights.len() {
                self.heights[i] = h;
            } else {
                self.heights.push(h);
            }
            if i < self.visual_heights.len() {
                self.visual_heights[i] = visual_h;
            } else {
                self.visual_heights.push(visual_h);
            }
            // Rebuild offset
            let prev = if i < self.offsets.len() {
                self.offsets[i]
            } else {
                *self.offsets.last().unwrap_or(&0)
            };
            let new_off = prev + h;
            if i + 1 < self.offsets.len() {
                self.offsets[i + 1] = new_off;
            } else {
                self.offsets.push(new_off);
            }

            let prev_visual = if i < self.visual_offsets.len() {
                self.visual_offsets[i]
            } else {
                *self.visual_offsets.last().unwrap_or(&0)
            };
            let new_visual_off = prev_visual + visual_h;
            if i + 1 < self.visual_offsets.len() {
                self.visual_offsets[i + 1] = new_visual_off;
            } else {
                self.visual_offsets.push(new_visual_off);
            }
        }
        // Make sure offsets has exactly total+1 entries
        self.offsets.truncate(total + 1);
        self.visual_offsets.truncate(total + 1);
        self.valid_up_to = total;
    }

    /// Total rendered line count across all messages.
    pub fn total_lines(&self) -> usize {
        self.offsets.last().copied().unwrap_or(0)
    }

    /// Total wrapped visual line count across all messages at the cached
    /// width. This is the layout leader-side render code should use for a
    /// true width-aware transcript/focus view.
    pub fn total_visual_lines(&self) -> usize {
        self.visual_offsets.last().copied().unwrap_or(0)
    }

    /// Compute the visible message index range `[start, end)` for the given
    /// scroll offset and viewport height.
    pub fn visible_range(&self, scroll_offset: usize, viewport_height: usize) -> (usize, usize) {
        visible_range_in_offsets(
            &self.offsets,
            self.heights.len(),
            scroll_offset,
            viewport_height,
        )
    }

    /// Width-aware visible range computed from wrapped visual heights.
    pub fn visual_range(&self, scroll_offset: usize, viewport_height: usize) -> (usize, usize) {
        visible_range_in_offsets(
            &self.visual_offsets,
            self.visual_heights.len(),
            scroll_offset,
            viewport_height,
        )
    }

    /// Line offset of message `index` in the wrapped visual line space.
    pub fn visual_offset_of(&self, index: usize) -> usize {
        self.visual_offsets.get(index).copied().unwrap_or(0)
    }

    /// Wrapped visual line count for a single cached message.
    pub fn visual_height_of(&self, index: usize) -> usize {
        self.visual_heights.get(index).copied().unwrap_or(0)
    }

    /// Terminal width used by the current cache. Exposed for tests /
    /// diagnostics that want to verify the cache reacted to a resize.
    pub fn cached_width(&self) -> u16 {
        self.cached_width
    }
}

fn visible_range_in_offsets(
    offsets: &[usize],
    total_msgs: usize,
    scroll_offset: usize,
    viewport_height: usize,
) -> (usize, usize) {
    if total_msgs == 0 {
        return (0, 0);
    }

    let lo = scroll_offset.saturating_sub(OVERSCAN);
    let hi = scroll_offset + viewport_height + OVERSCAN;

    // Binary search: first message whose cumulative end > lo
    let start = offsets.partition_point(|&o| o <= lo).saturating_sub(1);
    // First message whose cumulative start >= hi
    let end = offsets.partition_point(|&o| o < hi).min(total_msgs);

    (start, end)
}

/// Sum the wrapped display height for a rendered message body.
fn wrapped_line_height(lines: &[Line<'_>], width: u16) -> usize {
    let width = usize::from(width.max(1));
    lines
        .iter()
        .map(|line| rendered_line_width(line).max(1).div_ceil(width))
        .sum()
}

fn rendered_line_width(line: &Line<'_>) -> usize {
    line.spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

impl VirtualScroll {
    /// Line offset of message `index` in the global logical line space.
    pub fn offset_of(&self, index: usize) -> usize {
        self.offsets.get(index).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Theme;
    use allthecodes_types::message::{
        AssistantMessage, ContentBlock, Message, MessageContent, UserMessage,
    };

    fn user(text: &str) -> Message {
        Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "user".into(),
            content: MessageContent::Text(text.into()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    fn assistant_text(text: &str) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: vec![ContentBlock::Text { text: text.into() }],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
    }

    /// Width-aware cache data should reflect real visual reflow on resize,
    /// even before the shared renderer is switched over to the new visual
    /// offsets by the leader.
    #[test]
    fn width_change_recomputes_visual_heights() {
        let theme = Theme::default();
        let msgs = vec![
            user("a reasonably long message that will wrap differently on narrow widths"),
            assistant_text("and a long response that should also reflow"),
        ];
        let mut vs = VirtualScroll::new();
        let context = crate::ui::messages::build_message_render_context(&msgs, None, false);
        vs.ensure_up_to_date(&msgs, 80, &theme, &context);
        let logical_total = vs.total_lines();
        let visual_total_wide = vs.total_visual_lines();
        assert_eq!(vs.cached_width(), 80);
        assert!(logical_total > 0);

        // Width change -> wrapped heights should grow while logical heights
        // remain stable for the existing renderer contract.
        let context = crate::ui::messages::build_message_render_context(&msgs, None, false);
        vs.ensure_up_to_date(&msgs, 30, &theme, &context);
        assert_eq!(vs.cached_width(), 30);
        assert_eq!(vs.total_lines(), logical_total);
        assert!(vs.total_visual_lines() > visual_total_wide);
        assert!(vs.visual_offset_of(1) >= vs.offset_of(1));
        assert!(vs.visual_height_of(0) >= 2);

        // Same width again -> stable totals and offsets.
        let total_before_noop = vs.total_visual_lines();
        let context = crate::ui::messages::build_message_render_context(&msgs, None, false);
        vs.ensure_up_to_date(&msgs, 30, &theme, &context);
        assert_eq!(vs.total_visual_lines(), total_before_noop);
    }

    #[test]
    fn visual_range_uses_wrapped_offsets() {
        let theme = Theme::default();
        let msgs = vec![
            user("this message wraps a lot on narrow widths and should consume more than one line"),
            assistant_text("short"),
        ];
        let mut vs = VirtualScroll::new();
        let context = crate::ui::messages::build_message_render_context(&msgs, None, false);
        vs.ensure_up_to_date(&msgs, 18, &theme, &context);

        let first_visual_height = vs.visual_height_of(0);
        assert!(first_visual_height > 2);
        let before_boundary = vs.visual_range(first_visual_height.saturating_sub(1), 1);
        assert_eq!(before_boundary.0, 0);
        assert!(before_boundary.1 >= 1);
        let after_boundary = vs.visual_range(first_visual_height, 1);
        assert!(after_boundary.0 <= 1);
        assert!(after_boundary.1 >= 1);
    }

    #[test]
    fn logical_visible_range_uses_unwrapped_offsets() {
        let theme = Theme::default();
        let msgs = vec![user("first"), assistant_text("second")];
        let mut vs = VirtualScroll::new();
        let context = crate::ui::messages::build_message_render_context(&msgs, None, false);
        vs.ensure_up_to_date(&msgs, 80, &theme, &context);

        let range = vs.visible_range(0, 1);

        assert_eq!(range.0, 0);
        assert!(range.1 >= 1);
    }

    #[test]
    fn invalidate_all_resets_cache_to_empty() {
        let theme = Theme::default();
        let msgs = vec![user("hello")];
        let mut vs = VirtualScroll::new();
        let context = crate::ui::messages::build_message_render_context(&msgs, None, false);
        vs.ensure_up_to_date(&msgs, 40, &theme, &context);
        assert!(vs.total_lines() > 0);
        vs.invalidate_all();
        assert_eq!(vs.total_lines(), 0);
    }
}
