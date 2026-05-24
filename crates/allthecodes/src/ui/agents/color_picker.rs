//! Deterministic text model for the agent color picker.

use super::utils::selection_marker;

pub const COLOR_OPTIONS: &[&str] = &[
    "automatic",
    "red",
    "orange",
    "yellow",
    "green",
    "cyan",
    "blue",
    "purple",
    "pink",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorPickerState {
    pub agent_name: String,
    pub selected_index: usize,
}

impl ColorPickerState {
    pub fn new(agent_name: impl Into<String>, current_color: Option<&str>) -> Self {
        let current = current_color.unwrap_or("automatic");
        let selected_index = COLOR_OPTIONS
            .iter()
            .position(|option| *option == current)
            .unwrap_or(0);
        Self {
            agent_name: agent_name.into(),
            selected_index,
        }
    }

    pub fn move_next(&mut self) {
        self.selected_index = (self.selected_index + 1) % COLOR_OPTIONS.len();
    }
    pub fn move_previous(&mut self) {
        self.selected_index = if self.selected_index == 0 {
            COLOR_OPTIONS.len() - 1
        } else {
            self.selected_index - 1
        };
    }

    pub fn selected_color(&self) -> Option<&'static str> {
        let color = COLOR_OPTIONS[self.selected_index.min(COLOR_OPTIONS.len() - 1)];
        if color == "automatic" {
            None
        } else {
            Some(color)
        }
    }

    pub fn render(&self) -> String {
        let mut lines = Vec::new();
        for (idx, option) in COLOR_OPTIONS.iter().enumerate() {
            let label = if *option == "automatic" {
                "Automatic color".to_string()
            } else {
                capitalize(option)
            };
            lines.push(format!(
                "{} {}",
                selection_marker(idx == self.selected_index),
                label
            ));
        }

        let preview_color = self.selected_color().unwrap_or("automatic");
        lines.push(format!("preview: @{} ({preview_color})", self.agent_name));
        lines.join("\n")
    }
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
        None => String::new(),
    }
}
