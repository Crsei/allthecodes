//! Model selection helpers for agent editing and creation.

use super::utils::selection_marker;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOption {
    pub value: String,
    pub label: String,
    pub description: String,
}

impl ModelOption {
    pub fn new(
        value: impl Into<String>,
        label: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            description: description.into(),
        }
    }
}

pub fn default_model_options() -> Vec<ModelOption> {
    vec![
        ModelOption::new("MOTA", "MOTA", "Balanced capability and speed"),
        ModelOption::new("SOTA", "SOTA", "Highest capability work"),
        ModelOption::new("FOTA", "FOTA", "Fast lightweight work"),
    ]
}

pub fn model_options_with_current(initial_model: Option<&str>) -> Vec<ModelOption> {
    let mut options = default_model_options();
    if let Some(model) = initial_model {
        if !options.iter().any(|option| option.value == model) {
            options.insert(
                0,
                ModelOption::new(model, model, "Current model (custom ID)"),
            );
        }
    }
    options
}

pub fn render_model_selector(initial_model: Option<&str>) -> String {
    let default_value = initial_model.unwrap_or("MOTA");
    let options = model_options_with_current(initial_model);
    let mut lines =
        vec!["Model determines the agent's reasoning capabilities and speed.".to_string()];
    for option in options {
        lines.push(format!(
            "{} {:<12} {}",
            selection_marker(option.value == default_value),
            option.label,
            option.description
        ));
    }
    lines.join("\n")
}
