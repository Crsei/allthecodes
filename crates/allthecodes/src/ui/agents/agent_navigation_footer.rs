//! Footer text shared by agent menus.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentNavigationFooter {
    pub can_create: bool,
    pub can_edit: bool,
    pub can_delete: bool,
    pub in_selection: bool,
}

impl AgentNavigationFooter {
    pub fn render(&self) -> String {
        let mut hints = vec!["Up/Down navigate", "Enter select", "Esc back"];
        if self.can_create {
            hints.push("n new");
        }
        if self.can_edit {
            hints.push("e edit");
        }
        if self.can_delete {
            hints.push("d delete");
        }
        if self.in_selection {
            hints.push("Space toggle");
        }
        hints.join(" | ")
    }
}
