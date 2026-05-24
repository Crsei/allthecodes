// test infrastructure — slash-command helpers not called from production tests
//! Slash-command descriptors for command palette and prompt completion.

use allthecodes_commands as commands;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCommand {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
}

impl SlashCommand {
    pub fn label(&self) -> String {
        format!("/{}", self.name)
    }

    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().trim_start_matches('/').to_ascii_lowercase();
        if query.is_empty() {
            return true;
        }
        self.name.to_ascii_lowercase().contains(&query)
            || self
                .aliases
                .iter()
                .any(|alias| alias.to_ascii_lowercase().contains(&query))
            || self.description.to_ascii_lowercase().contains(&query)
    }
}

pub fn built_in_slash_commands() -> Vec<SlashCommand> {
    commands::get_all_commands()
        .into_iter()
        .map(|command| SlashCommand {
            name: command.name,
            aliases: command.aliases,
            description: command.description,
        })
        .collect()
}

pub fn find_slash_command(query: &str) -> Option<SlashCommand> {
    let query = query.trim().trim_start_matches('/');
    built_in_slash_commands()
        .into_iter()
        .find(|command| command.name == query || command.aliases.iter().any(|alias| alias == query))
}

pub fn filter_slash_commands(query: &str) -> Vec<SlashCommand> {
    built_in_slash_commands()
        .into_iter()
        .filter(|command| command.matches(query))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_help_command() {
        assert!(find_slash_command("/help").is_some());
    }

    #[test]
    fn filters_and_labels_commands() {
        let commands = filter_slash_commands("he");
        let help = commands
            .iter()
            .find(|command| command.name == "help")
            .expect("help command should match he");

        assert_eq!(help.label(), "/help");
        assert!(help.matches("/he"));
    }
}
