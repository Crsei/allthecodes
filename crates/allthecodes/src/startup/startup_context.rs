use std::path::PathBuf;

use anyhow::Context;
use tracing::info;

use crate::cli::Cli;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupMode {
    InitOnly,
    Json,
    Print,
    Server,
    Headless,
    Acp,
    Tui,
}

pub(crate) struct StartupContext {
    pub(crate) cli: Cli,
    pub(crate) cwd: PathBuf,
    pub(crate) initial_prompt: Option<String>,
    pub(crate) mode: StartupMode,
    pub(crate) chrome_enabled: bool,
    pub(crate) computer_use_enabled: bool,
}

impl StartupContext {
    pub(crate) async fn from_cli(cli: Cli) -> anyhow::Result<Self> {
        let cwd = PathBuf::from(allthecodes_startup::runtime_config::resolve_cwd(&cli));

        // If -C / --cwd was given, switch the process working directory so
        // tools operate in the target workspace, matching the former
        // full_init.rs composition root behavior.
        if cli.cwd.is_some() {
            if cwd.is_dir() {
                std::env::set_current_dir(&cwd).with_context(|| {
                    format!("failed to set working directory to {}", cwd.display())
                })?;
                info!(cwd = %cwd.display(), "working directory changed via --cwd");
            } else {
                anyhow::bail!(
                    "--cwd path does not exist or is not a directory: {}",
                    cwd.display()
                );
            }
        }

        let initial_prompt = if cli.prompt.is_empty() {
            None
        } else {
            Some(cli.prompt.join(" "))
        };
        let mode = StartupMode::from_cli(&cli);
        let chrome_enabled =
            allthecodes_startup::runtime_config::chrome_cli_override(&cli).unwrap_or(false);
        let computer_use_enabled = cli.computer_use;

        Ok(Self {
            cli,
            cwd,
            initial_prompt,
            mode,
            chrome_enabled,
            computer_use_enabled,
        })
    }
}

impl StartupMode {
    fn from_cli(cli: &Cli) -> Self {
        if cli.init_only {
            Self::InitOnly
        } else if cli.output_format.as_deref() == Some("json") {
            Self::Json
        } else if cli.print {
            Self::Print
        } else if cli.listen.is_some() || cli.web || cli.daemon {
            Self::Server
        } else if cli.headless {
            Self::Headless
        } else if cli.acp {
            Self::Acp
        } else {
            Self::Tui
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StartupMode;
    use crate::cli::Cli;
    use clap::Parser;

    #[test]
    fn json_mode_precedes_print_mode() {
        let cli = Cli::parse_from(["claude", "-p", "--output-format", "json"]);

        assert_eq!(StartupMode::from_cli(&cli), StartupMode::Json);
    }
}
