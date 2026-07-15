//! `/experimental` command -- inspect and override experimental feature gates.
//!
//! Environment flags are still the startup default. This command adds a
//! session-local override so TUI users can opt in from the slash-command
//! surface without restarting.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_config::features::{self, FeatureFlags};

pub struct ExperimentalHandler;

#[async_trait]
impl CommandHandler for ExperimentalHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let subcommand = args.trim().to_ascii_lowercase();

        match subcommand.as_str() {
            "" | "status" => Ok(CommandResult::Output(status_text())),
            "list" => Ok(CommandResult::Output(list_text())),
            "on" | "enable" => {
                features::set_runtime_override(FeatureFlags::all_enabled());
                Ok(CommandResult::Output(format!(
                    "Experimental features enabled for this TUI session.\n\
                     This does not persist KAIROS settings or start the KAIROS daemon; use /kairos enable --start for that.\n\
                     Startup-only background processes may still require a restart.\n\n{}",
                    list_text()
                )))
            }
            "off" | "disable" => {
                features::set_runtime_override(FeatureFlags::all_disabled());
                Ok(CommandResult::Output(format!(
                    "Experimental features disabled for this TUI session.\n\n{}",
                    list_text()
                )))
            }
            "reset" => {
                features::clear_runtime_override();
                Ok(CommandResult::Output(format!(
                    "Experimental features reset to startup environment flags.\n\n{}",
                    list_text()
                )))
            }
            "help" | "--help" | "-h" => Ok(CommandResult::Output(usage_text().to_string())),
            other => Ok(CommandResult::Output(format!(
                "Unknown experimental subcommand: '{}'\n{}",
                other,
                usage_text()
            ))),
        }
    }
}

fn usage_text() -> &'static str {
    "Usage: /experimental [status|list|on|off|reset]\n\
     \n\
       /experimental          Show effective experimental gate status\n\
       /experimental list     Show every known experimental gate\n\
       /experimental on       Enable every known experimental gate for this TUI session\n\
       /experimental off      Disable every known experimental gate for this TUI session\n\
       /experimental reset    Return to startup environment flags\n\
     \n\
     These overrides affect only this process and do not start the KAIROS daemon."
}

fn status_text() -> String {
    let source = if features::runtime_override().is_some() {
        "session override"
    } else {
        "startup environment"
    };

    format!(
        "Experimental features: {}\n\n{}",
        source,
        format_flags(&features::current())
    )
}

fn list_text() -> String {
    format_flags(&features::current())
}

fn format_flags(flags: &FeatureFlags) -> String {
    let mut lines = Vec::new();
    for descriptor in features::feature_descriptors() {
        let state = if flags.is_enabled(descriptor.feature) {
            "ON"
        } else {
            "off"
        };
        lines.push(format!(
            "  {:<28} {:<3} {} [{}]",
            descriptor.label, state, descriptor.description, descriptor.env_var
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_flags_lists_known_features() {
        let text = format_flags(&FeatureFlags::all_enabled());
        assert!(text.contains("kairos"));
        assert!(text.contains("agent_teams"));
        assert!(text.contains("ALLTHECODES_EXPERIMENTAL_AGENT_TEAMS"));
        assert!(text.contains("coordinator"));
        assert!(text.contains("ALLTHECODES_COORDINATOR_MODE"));
        assert!(text.contains("mcp_skills"));
        assert!(text.contains("FEATURE_MCP_SKILLS"));
        assert!(text.contains("experimental_skill_search"));
        assert!(text.contains("FEATURE_EXPERIMENTAL_SKILL_SEARCH"));
        assert!(text.contains("remote_url_discovery"));
        assert!(text.contains("FEATURE_REMOTE_URL_DISCOVERY"));
    }

    #[test]
    fn usage_mentions_runtime_subcommands() {
        let usage = usage_text();
        assert!(usage.contains("/experimental on"));
        assert!(usage.contains("/experimental reset"));
    }
}
