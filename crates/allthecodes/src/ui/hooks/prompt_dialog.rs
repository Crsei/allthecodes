//! Prompt dialog used while adding or editing a hook command.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptDialogState {
    pub title: String,
    pub prompt: String,
    pub input: String,
    pub error: Option<String>,
}

impl PromptDialogState {
    pub fn new(title: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            prompt: prompt.into(),
            input: String::new(),
            error: None,
        }
    }

    pub fn render(&self) -> String {
        let mut lines = vec![
            self.title.clone(),
            self.prompt.clone(),
            format!(
                "> {}",
                if self.input.is_empty() {
                    "<empty>"
                } else {
                    &self.input
                }
            ),
        ];
        if let Some(error) = &self.error {
            lines.push(format!("error: {error}"));
        }
        lines.push("Enter confirm | Esc cancel".to_string());
        lines.join("\n")
    }
}
