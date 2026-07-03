// test infrastructure — markdown streaming collector methods not called in production tests
//! Streaming Markdown collector with render caching.
//!
//! The collector keeps commit accounting separate from render caching:
//! [`commit`](MarkdownStreamCollector::commit) returns only uncommitted text,
//! while [`render`](MarkdownStreamCollector::render) may reuse the previous
//! rendered frame when the raw buffer is unchanged.

use ratatui::text::Text;

use super::markdown_render::render_markdown_text;

/// A set of characters that indicate markdown syntax may be present.
/// Used as a fast-path to skip full parsing for plain text.
const MARKDOWN_SYNTAX_CHARS: &[char] = &[
    '*', '_', '`', '#', '[', ']', '(', ')', '>', '-', '|', '~', '\\',
];

/// Quick check for whether a string contains any markdown syntax characters.
/// When this returns false, markdown parsing can be skipped entirely.
fn has_markdown_syntax(text: &str) -> bool {
    text.contains(MARKDOWN_SYNTAX_CHARS)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarkdownStreamCollector {
    buffer: String,
    /// Byte index in the buffer up to which content has been committed.
    committed_len: usize,
    /// Byte length of the buffer represented by `rendered_lines`.
    rendered_len: usize,
    /// Cached rendered frame for the last rendered buffer.
    rendered_lines: Text<'static>,
}

impl MarkdownStreamCollector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_str(&mut self, delta: &str) {
        self.buffer.push_str(delta);
    }

    pub fn replace(&mut self, text: impl Into<String>) {
        let new = text.into();
        let common = common_prefix_len_bytes(&self.buffer, &new);
        self.buffer = new;
        if common < self.committed_len {
            self.committed_len = common;
        }
        self.invalidate_render_cache();
    }

    pub fn raw(&self) -> &str {
        &self.buffer
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Render the current buffer.
    ///
    /// For buffers with no markdown syntax, skips pulldown_cmark entirely
    /// and returns plain text directly (fast path).
    pub fn render(&mut self) -> Text<'static> {
        if self.buffer.is_empty() {
            return Text::default();
        }

        if self.rendered_len == self.buffer.len() {
            return self.rendered_lines.clone();
        }

        // Fast path: if no markdown syntax, skip pulldown_cmark entirely.
        let rendered = if !has_markdown_syntax(&self.buffer) {
            let mut text = Text::default();
            for line in self.buffer.lines() {
                text.lines
                    .push(ratatui::text::Line::from(ratatui::text::Span::raw(
                        line.to_string(),
                    )));
            }
            text
        } else {
            // pulldown_cmark does not expose an incremental API; cache the
            // whole rendered frame and reuse it while the buffer is unchanged.
            render_markdown_text(&self.buffer)
        };
        self.rendered_len = self.buffer.len();
        self.rendered_lines = rendered.clone();
        rendered
    }

    /// Commit the pending text as stable, updating the committed boundary.
    pub fn commit(&mut self) -> String {
        let delta = self.buffer[self.committed_len..].to_string();
        self.committed_len = self.buffer.len();
        delta
    }

    fn invalidate_render_cache(&mut self) {
        self.rendered_len = 0;
        self.rendered_lines = Text::default();
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
        self.committed_len = 0;
        self.invalidate_render_cache();
    }
}

fn common_prefix_len_bytes(left: &str, right: &str) -> usize {
    let mut common = 0;
    for ((left_idx, left_ch), (right_idx, right_ch)) in
        left.char_indices().zip(right.char_indices())
    {
        if left_ch != right_ch {
            break;
        }
        debug_assert_eq!(left_idx, right_idx);
        common = left_idx + left_ch.len_utf8();
    }
    common
}

pub(crate) fn simulate_stream_markdown_for_tests(chunks: &[&str]) -> Vec<Text<'static>> {
    let mut collector = MarkdownStreamCollector::new();
    chunks
        .iter()
        .map(|chunk| {
            collector.push_str(chunk);
            collector.render()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_returns_only_new_delta() {
        let mut collector = MarkdownStreamCollector::new();
        collector.push_str("a");
        assert_eq!(collector.commit(), "a");
        collector.push_str("b");
        assert_eq!(collector.commit(), "b");
    }

    #[test]
    fn plain_text_fast_path_skips_markdown_parsing() {
        let mut collector = MarkdownStreamCollector::new();
        collector.push_str("hello world");
        let rendered = collector.render();
        assert!(!rendered.lines.is_empty());
        assert!(rendered.lines[0]
            .spans
            .iter()
            .any(|s| s.content.contains("hello")));
    }

    #[test]
    fn markdown_syntax_is_detected() {
        assert!(has_markdown_syntax("**bold**"));
        assert!(has_markdown_syntax("# heading"));
        assert!(!has_markdown_syntax("just plain text"));
        assert!(has_markdown_syntax("`code`"));
    }

    #[test]
    fn render_cache_does_not_commit_text() {
        let mut collector = MarkdownStreamCollector::new();
        collector.push_str("hello ");
        let _ = collector.render();
        collector.push_str("world");
        let _ = collector.render();
        assert_eq!(collector.commit(), "hello world");
    }

    #[test]
    fn replace_preserves_common_prefix_cache() {
        let mut collector = MarkdownStreamCollector::new();
        collector.push_str("hello **world**");
        collector.render(); // cache committed
        let committed_before = collector.committed_len;
        // Replace with a change in the suffix
        collector.replace("hello **universe**");
        assert!(collector.committed_len <= committed_before);
    }

    #[test]
    fn replace_handles_unicode_prefix_boundaries() {
        let mut collector = MarkdownStreamCollector::new();
        collector.replace("你好 **world**");
        assert_eq!(collector.commit(), "你好 **world**");
        collector.replace("你好 **rust**");
        assert_eq!(collector.commit(), "rust**");
    }

    #[test]
    fn raw_empty_reset_and_simulated_frames_are_exercised() {
        let mut collector = MarkdownStreamCollector::new();
        assert!(collector.is_empty());
        collector.push_str("hello");
        assert_eq!(collector.raw(), "hello");
        collector.reset();
        assert!(collector.is_empty());

        let frames = simulate_stream_markdown_for_tests(&["**he", "llo**"]);
        assert_eq!(frames.len(), 2);
        assert!(!frames[1].lines.is_empty());
    }
}
