//! `/coordinator` command -- activate coordinator mode and bind a team.

use allthecodes_config::features::{self, Feature, FeatureFlags};
use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};

pub struct CoordinatorHandler;

const COORDINATOR_CHAT_MODE: &str = "coordinator";

#[async_trait]
impl CommandHandler for CoordinatorHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let mut parts = args.trim().splitn(2, char::is_whitespace);
        let subcommand = parts.next().unwrap_or("").trim();
        let rest = parts.next().unwrap_or("").trim();

        let output = match subcommand {
            "" | "status" => Ok(status(ctx)),
            "start" | "on" => start(ctx, rest).await,
            "stop" | "off" => stop(ctx),
            "help" | "--help" | "-h" => Ok(usage().to_string()),
            other => Ok(format!(
                "Unknown /coordinator subcommand: '{}'\n{}",
                other,
                usage()
            )),
        }?;
        Ok(CommandResult::Output(output))
    }
}

fn usage() -> &'static str {
    "Usage: /coordinator [status|start|stop]\n\
     \n\
       /coordinator                  Show coordinator status\n\
       /coordinator start [team]     Enable coordinator mode and bind/create a team\n\
       /coordinator stop             Disable coordinator prompt/tool policy for this session"
}

fn status(ctx: &CommandContext) -> String {
    let coordinator_enabled = is_coordinator_mode_enabled();
    let mut lines = vec![format!(
        "Coordinator mode: {}",
        if coordinator_enabled { "ON" } else { "off" }
    )];

    if let Some(team) = ctx.app_state.team_context.as_ref() {
        lines.push(format!("Active team: {}", team.team_name));
        lines.push(format!("Lead: {}", team.lead_agent_id));
        lines.push(format!("Visible teammates: {}", team.teammates.len()));
        let snapshots = crate::runtime::team_task_snapshots();
        let running = snapshots
            .iter()
            .filter(|snapshot| snapshot.status == crate::runtime::TeamTaskStatus::Running)
            .count();
        let idle = snapshots
            .iter()
            .filter(|snapshot| {
                snapshot.status == crate::runtime::TeamTaskStatus::Running && snapshot.is_idle
            })
            .count();
        lines.push(format!(
            "Team task status: {} running ({} idle), {} total",
            running,
            idle,
            snapshots.len()
        ));
    } else {
        lines.push("Active team: none".to_string());
    }

    lines.push(format!(
        "Tool policy: {}",
        tool_policy_summary(coordinator_enabled)
    ));
    lines.join("\n")
}

async fn start(ctx: &mut CommandContext, rest: &str) -> Result<String> {
    set_session_coordinator_mode(ctx, true)?;

    let team_name = rest
        .split_whitespace()
        .next()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("coordinator");

    if ctx
        .app_state
        .team_context
        .as_ref()
        .map(|team| team.team_name == team_name)
        .unwrap_or(false)
    {
        return Ok(format!(
            "Coordinator mode enabled for existing active team '{}'.",
            team_name
        ));
    }

    let output = crate::runtime::execute_team_command(
        &format!("create {} Coordinator mode team", team_name),
        ctx,
    )
    .await;
    if output.starts_with("Failed")
        || output.starts_with("Usage:")
        || output == "Team command runtime is unavailable."
    {
        return Ok(format!("Failed to create coordinator team: {output}"));
    }

    let Some(team_name) = ctx
        .app_state
        .team_context
        .as_ref()
        .map(|team| team.team_name.clone())
        .filter(|name| !name.is_empty())
    else {
        return Ok(format!(
            "Coordinator mode enabled, but no active team was returned by the team runtime: {output}"
        ));
    };

    Ok(format!(
        "Coordinator mode enabled and team '{}' is active. Spawn workers with Agent(name=..., prompt=...) or /team spawn.",
        team_name
    ))
}

fn stop(ctx: &mut CommandContext) -> Result<String> {
    set_session_coordinator_mode(ctx, false)?;
    Ok(
        "Coordinator mode disabled for this session. Active team state was left intact."
            .to_string(),
    )
}

fn is_coordinator_mode_enabled() -> bool {
    features::enabled(Feature::Coordinator)
}

fn set_coordinator_mode_enabled(enabled: bool) {
    let mut flags: FeatureFlags = features::current();
    flags.coordinator = enabled;
    if enabled {
        flags.agent_teams = true;
    }
    features::set_runtime_override(flags);
}

fn set_session_coordinator_mode(ctx: &CommandContext, enabled: bool) -> Result<()> {
    let value = enabled.then_some(COORDINATOR_CHAT_MODE);
    allthecodes_session::storage::set_session_chat_mode_override(
        ctx.session_id.as_str(),
        value,
        ctx.cwd.to_string_lossy().as_ref(),
    )?;
    set_coordinator_mode_enabled(enabled);
    Ok(())
}

fn tool_policy_summary(coordinator_enabled: bool) -> String {
    let policy = if coordinator_enabled {
        crate::runtime::CommandToolPolicy::Coordinator
    } else {
        crate::runtime::CommandToolPolicy::DefaultAgent
    };
    let mut names = crate::runtime::tool_policy_names(policy);
    names.sort_unstable();
    if names.is_empty() {
        "runtime tool-name adapter unavailable".to_string()
    } else {
        names.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_config::features::{self, FeatureFlags};
    use std::path::PathBuf;

    const TEAM_LEAD_NAME: &str = "team-lead";

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    struct FeatureOverrideGuard;

    impl Drop for FeatureOverrideGuard {
        fn drop(&mut self) {
            features::clear_runtime_override();
        }
    }

    fn make_ctx(cwd: PathBuf) -> CommandContext {
        CommandContext {
            messages: vec![],
            cwd,
            app_state: Default::default(),
            session_id: SessionId::from_string("coordinator-test-session"),
        }
    }

    fn install_mock_team_runtime() {
        crate::runtime::set_team_command_executor(mock_team_command);
        crate::runtime::set_tool_policy_names_provider(mock_tool_policy_names);
    }

    fn mock_tool_policy_names(policy: crate::runtime::CommandToolPolicy) -> Vec<String> {
        match policy {
            crate::runtime::CommandToolPolicy::DefaultAgent => vec!["Bash".to_string()],
            crate::runtime::CommandToolPolicy::Coordinator => {
                vec!["Agent".to_string(), "SendMessage".to_string()]
            }
        }
    }

    fn mock_team_command<'a>(
        args: &'a str,
        ctx: &'a mut CommandContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = String> + Send + 'a>> {
        Box::pin(async move {
            let mut parts = args.trim().splitn(3, char::is_whitespace);
            let subcommand = parts.next().unwrap_or("");
            let team_name = parts.next().unwrap_or("").trim();
            if subcommand != "create" || team_name.is_empty() {
                return "mock team runtime only supports create".to_string();
            }

            let lead_agent_id = format!("{TEAM_LEAD_NAME}@{team_name}");
            ctx.app_state.team_context = Some(allthecodes_types::teams::TeamContext {
                team_name: team_name.to_string(),
                team_file_path: ctx
                    .cwd
                    .join(".allthecodes")
                    .join("teams")
                    .join(team_name)
                    .join("config.json")
                    .to_string_lossy()
                    .into_owned(),
                lead_agent_id: lead_agent_id.clone(),
                self_agent_id: Some(lead_agent_id),
                self_agent_name: Some(TEAM_LEAD_NAME.to_string()),
                is_leader: Some(true),
                self_agent_color: None,
                teammates: Default::default(),
            });

            format!("Team '{}' created and activated.", team_name)
        })
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn start_enables_coordinator_and_creates_active_team() {
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let _features = FeatureOverrideGuard;
        features::set_runtime_override(FeatureFlags::all_disabled());
        install_mock_team_runtime();

        let mut ctx = make_ctx(cwd.path().to_path_buf());
        let result = CoordinatorHandler
            .execute("start alpha", &mut ctx)
            .await
            .unwrap();

        let CommandResult::Output(text) = result else {
            panic!("expected output");
        };
        assert!(text.contains("Coordinator mode enabled"));
        assert!(is_coordinator_mode_enabled());
        let info = allthecodes_session::storage::load_session_info(ctx.session_id.as_str())
            .expect("coordinator start persists session metadata");
        assert_eq!(
            info.chat_mode_override.as_deref(),
            Some(COORDINATOR_CHAT_MODE)
        );
        assert_eq!(
            ctx.app_state
                .team_context
                .as_ref()
                .map(|team| team.team_name.as_str()),
            Some("alpha")
        );
        assert_eq!(
            ctx.app_state
                .team_context
                .as_ref()
                .and_then(|team| team.self_agent_name.as_deref()),
            Some(TEAM_LEAD_NAME)
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn stop_disables_coordinator_without_clearing_team() {
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let _features = FeatureOverrideGuard;
        features::set_runtime_override(FeatureFlags::all_disabled());
        install_mock_team_runtime();

        let mut ctx = make_ctx(cwd.path().to_path_buf());
        CoordinatorHandler
            .execute("start beta", &mut ctx)
            .await
            .unwrap();
        let result = CoordinatorHandler.execute("stop", &mut ctx).await.unwrap();

        let CommandResult::Output(text) = result else {
            panic!("expected output");
        };
        assert!(text.contains("disabled"));
        assert!(!is_coordinator_mode_enabled());
        let info = allthecodes_session::storage::load_session_info(ctx.session_id.as_str())
            .expect("coordinator stop persists session metadata");
        assert_eq!(info.chat_mode_override, None);
        assert!(ctx.app_state.team_context.is_some());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn status_reports_coordinator_tool_policy() {
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let _features = FeatureOverrideGuard;
        features::set_runtime_override(FeatureFlags::all_disabled());
        install_mock_team_runtime();

        let mut ctx = make_ctx(cwd.path().to_path_buf());
        CoordinatorHandler
            .execute("start gamma", &mut ctx)
            .await
            .unwrap();
        let result = CoordinatorHandler
            .execute("status", &mut ctx)
            .await
            .unwrap();

        let CommandResult::Output(text) = result else {
            panic!("expected output");
        };
        assert!(text.contains("Coordinator mode: ON"));
        assert!(text.contains("Active team: gamma"));
        assert!(text.contains("Agent"));
        assert!(text.contains("SendMessage"));
        assert!(!text.contains("Bash"));
    }
}
