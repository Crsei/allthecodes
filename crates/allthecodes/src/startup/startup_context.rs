use std::path::PathBuf;

use allthecodes_browser::session::ChromeEnablement;
use allthecodes_types::permissions::PermissionMode;
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
    pub(crate) chrome_enablement: ChromeEnablement,
    pub(crate) computer_use_enabled: bool,
    pub(crate) permission_mode: Option<PermissionMode>,
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
        let chrome_enablement = allthecodes_browser::session::resolve_enablement(
            allthecodes_startup::runtime_config::chrome_cli_override(&cli),
            None,
        );
        let chrome_enabled = chrome_enablement.is_enabled();
        let computer_use_enabled = cli.computer_use;

        Ok(Self {
            cli,
            cwd,
            initial_prompt,
            mode,
            chrome_enabled,
            chrome_enablement,
            computer_use_enabled,
            permission_mode: None,
        })
    }

    pub(crate) fn resolve_runtime_decisions(
        &mut self,
        config_permission_mode: Option<&str>,
        chrome_config_default: Option<bool>,
    ) -> anyhow::Result<()> {
        let permission_mode = allthecodes_startup::runtime_config::resolve_permission_mode(
            self.cli.permission_mode.as_deref(),
            config_permission_mode,
        )?;
        let chrome_enablement = allthecodes_browser::session::resolve_enablement(
            allthecodes_startup::runtime_config::chrome_cli_override(&self.cli),
            chrome_config_default,
        );

        self.permission_mode = Some(permission_mode);
        self.chrome_enablement = chrome_enablement;
        self.chrome_enabled = chrome_enablement.is_enabled();
        self.computer_use_enabled = self.cli.computer_use;
        Ok(())
    }

    pub(crate) fn permission_mode(&self) -> PermissionMode {
        self.permission_mode
            .clone()
            .unwrap_or(PermissionMode::Default)
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

    #[test]
    fn resolved_permission_mode_is_recorded_on_startup_context() {
        let cli = Cli::parse_from(["claude", "--permission-mode", "bypass"]);
        let mut startup = super::StartupContext {
            cli,
            cwd: std::path::PathBuf::from("."),
            initial_prompt: None,
            mode: StartupMode::Tui,
            chrome_enabled: false,
            chrome_enablement: allthecodes_browser::session::ChromeEnablement::Disabled,
            computer_use_enabled: false,
            permission_mode: None,
        };
        startup
            .resolve_runtime_decisions(None, Some(false))
            .expect("permission mode resolves");

        assert_eq!(
            startup.permission_mode,
            Some(allthecodes_types::permissions::PermissionMode::Bypass)
        );
    }
}
