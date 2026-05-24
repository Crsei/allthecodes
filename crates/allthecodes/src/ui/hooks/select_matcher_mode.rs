//! Hook matcher selection and rendering.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookMatcher {
    pub matcher: String,
    pub sources: Vec<String>,
    pub hook_count: usize,
}

impl HookMatcher {
    pub fn all_tools() -> Self {
        Self {
            matcher: String::new(),
            sources: Vec::new(),
            hook_count: 0,
        }
    }

    pub fn for_tool(tool_name: impl Into<String>) -> Self {
        Self {
            matcher: tool_name.into(),
            sources: Vec::new(),
            hook_count: 0,
        }
    }

    pub fn label(&self) -> String {
        match self.matcher.as_str() {
            "" => "(all)".to_string(),
            "*" => "All tools (*)".to_string(),
            matcher => matcher.to_string(),
        }
    }

    pub fn source_label(&self) -> String {
        if self.sources.is_empty() {
            "Effective".to_string()
        } else {
            self.sources.join(", ")
        }
    }
}

pub fn render_select_matcher_mode(
    event_name: &str,
    event_description: &str,
    matchers: &[HookMatcher],
    selected_index: usize,
) -> String {
    let mut lines = vec![format!("{event_name} - Matchers")];
    lines.extend(event_description.lines().map(str::to_string));
    lines.push(String::new());
    if matchers.is_empty() {
        lines.push("No hooks configured for this event.".to_string());
        lines.push("To add hooks, edit .allthecodes/settings.json or ask Claude.".to_string());
        return lines.join("\n");
    }
    for (idx, matcher) in matchers.iter().enumerate() {
        let marker = if idx == selected_index { ">" } else { " " };
        let hooks = match matcher.hook_count {
            1 => "1 hook".to_string(),
            count => format!("{count} hooks"),
        };
        lines.push(format!(
            "{marker} [{}] {:<18} {}",
            matcher.source_label(),
            matcher.label(),
            hooks
        ));
    }
    lines.join("\n")
}
