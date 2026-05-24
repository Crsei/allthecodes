//! MCP elicitation prompt rendering.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElicitationField {
    pub name: String,
    pub prompt: String,
    pub value: String,
    pub required: bool,
}

pub fn render_elicitation_dialog(
    title: &str,
    fields: &[ElicitationField],
    selected_index: usize,
) -> String {
    let mut lines = vec![title.to_string()];
    for (idx, field) in fields.iter().enumerate() {
        let marker = if idx == selected_index { ">" } else { " " };
        let required = if field.required {
            "required"
        } else {
            "optional"
        };
        let value = if field.value.is_empty() {
            "<empty>"
        } else {
            &field.value
        };
        lines.push(format!(
            "{marker} {} ({required}) - {}",
            field.name, field.prompt
        ));
        lines.push(format!("  value: {value}"));
    }
    lines.push("Enter submit | Esc cancel".to_string());
    lines.join("\n")
}
