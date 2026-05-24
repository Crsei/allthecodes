// test infrastructure — streaming controller not wired to TUI streaming path yet
//! Deterministic streaming controller.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamingDelta {
    Assistant(String),
    Thinking(String),
    ToolCall(String),
    Final(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StreamingController {
    assistant: String,
    thinking: String,
    tool_call: Option<String>,
    committed_chars: usize,
    complete: bool,
}

impl StreamingController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, delta: StreamingDelta) {
        match delta {
            StreamingDelta::Assistant(text) => self.assistant.push_str(&text),
            StreamingDelta::Thinking(text) => self.thinking.push_str(&text),
            StreamingDelta::ToolCall(text) => self.tool_call = Some(text),
            StreamingDelta::Final(text) => {
                self.assistant = text;
                self.complete = true;
            }
        }
    }

    pub fn commit_visible(&mut self) -> &str {
        self.committed_chars = self.assistant.chars().count();
        &self.assistant
    }

    pub fn render_lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "stream complete={} committed={}",
            self.complete, self.committed_chars
        )];
        if !self.thinking.is_empty() {
            lines.push(format!("thinking: {}", self.thinking));
        }
        if let Some(tool) = &self.tool_call {
            lines.push(format!("tool-delta: {tool}"));
        }
        lines.push(format!("assistant: {}", self.assistant));
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_delta_replaces_assistant_and_marks_complete() {
        let mut controller = StreamingController::new();
        controller.apply(StreamingDelta::Assistant("partial".to_string()));
        controller.apply(StreamingDelta::Final("complete".to_string()));

        assert_eq!(controller.commit_visible(), "complete");
        let lines = controller.render_lines().join("\n");
        assert!(lines.contains("stream complete=true"));
        assert!(lines.contains("assistant: complete"));
    }
}
