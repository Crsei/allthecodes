//! Multiple-choice state for ask-user-question permission prompts.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipleChoiceState {
    pub options: Vec<String>,
    pub selected: usize,
    pub submitted: Vec<usize>,
}

impl MultipleChoiceState {
    pub fn new(options: Vec<String>) -> Self {
        Self {
            options,
            selected: 0,
            submitted: Vec::new(),
        }
    }

    pub fn select_next(&mut self) {
        if !self.options.is_empty() {
            self.selected = (self.selected + 1) % self.options.len();
        }
    }

    pub fn toggle_selected(&mut self) {
        if self.options.is_empty() {
            return;
        }
        if let Some(index) = self.submitted.iter().position(|idx| *idx == self.selected) {
            self.submitted.remove(index);
        } else {
            self.submitted.push(self.selected);
            self.submitted.sort_unstable();
        }
    }
}

pub fn render_multiple_choice_state(state: &MultipleChoiceState) -> String {
    state
        .options
        .iter()
        .enumerate()
        .map(|(idx, option)| {
            let marker = if idx == state.selected { ">" } else { " " };
            let checked = if state.submitted.contains(&idx) {
                "x"
            } else {
                " "
            };
            format!("{marker} [{checked}] {option}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
