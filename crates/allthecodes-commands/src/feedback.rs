//! `/feedback` slash command: open the product feedback page.

use std::process::Command;

use anyhow::Result;
use async_trait::async_trait;

use super::{CommandContext, CommandHandler, CommandResult};

const FEEDBACK_BASE_URL: &str = "https://allthecodes.com/feedback";

pub struct FeedbackHandler;

#[async_trait]
impl CommandHandler for FeedbackHandler {
    async fn execute(&self, _args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let url = feedback_url();
        let output = match open_feedback_url(&url) {
            Ok(()) => format!("Opened the allthecodes feedback page:\n{url}"),
            Err(error) => format!(
                "Unable to open the allthecodes feedback page automatically: {error}\n\nOpen this URL in your browser:\n{url}"
            ),
        };

        Ok(CommandResult::Output(output))
    }
}

fn feedback_url() -> String {
    format!(
        "{FEEDBACK_BASE_URL}?source=tui&version={}",
        env!("CARGO_PKG_VERSION")
    )
}

fn open_feedback_url(url: &str) -> Result<(), String> {
    let status = open_command(url)
        .status()
        .map_err(|error| error.to_string())?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("browser opener exited with {status}"))
    }
}

#[cfg(target_os = "macos")]
fn open_command(url: &str) -> Command {
    let mut command = Command::new("open");
    command.arg(url);
    command
}

#[cfg(target_os = "windows")]
fn open_command(url: &str) -> Command {
    let mut command = Command::new("cmd");
    command.args(["/C", "start", "", url]);
    command
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn open_command(url: &str) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(url);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_url_points_to_product_feedback_with_tui_source() {
        let url = feedback_url();

        assert!(url.starts_with("https://allthecodes.com/feedback?"));
        assert!(url.contains("source=tui"));
        assert!(url.contains(env!("CARGO_PKG_VERSION")));
    }
}
