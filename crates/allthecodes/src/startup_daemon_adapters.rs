use std::sync::Arc;

use allthecodes_startup::tool_registry as registry;
use serde_json::Value;

pub(crate) fn install_daemon_runtime_adapters() {
    allthecodes_daemon::runtime::set_runtime_adapters(
        allthecodes_daemon::runtime::DaemonRuntimeAdapters {
            init_plugins: allthecodes_plugins::init_plugins,
            active_tools: registry::get_tools_for_active_session,
            commands: allthecodes_commands::get_all_commands,
            command_dispatcher: daemon_command_dispatcher,
            command_executor: daemon_command_executor,
            route_github_pr_activity: daemon_route_github_pr_activity,
        },
    );
}

pub(crate) fn daemon_command_dispatcher() -> Arc<dyn allthecodes_types::commands::CommandDispatcher>
{
    Arc::new(allthecodes_commands::DefaultCommandDispatcher::for_full_registry())
}

pub(crate) fn daemon_command_executor(
) -> Arc<dyn allthecodes_engine::command_runtime::CommandExecutor> {
    Arc::new(allthecodes_commands::EngineCommandExecutor)
}

pub(crate) fn daemon_route_github_pr_activity(
    payload: &Value,
    event: Option<&str>,
    delivery_id: Option<&str>,
) -> anyhow::Result<Option<allthecodes_daemon::runtime::GithubPrActivityRouteOutcome>> {
    let Some(activity) =
        allthecodes_teams::pr_activity::parse_github_pr_activity(payload, event, delivery_id)
    else {
        return Ok(None);
    };
    let result = allthecodes_teams::pr_activity::route_github_pr_activity(&activity)?;
    Ok(Some(
        allthecodes_daemon::runtime::GithubPrActivityRouteOutcome {
            matched: result.matched,
            delivered: result.delivered,
        },
    ))
}
