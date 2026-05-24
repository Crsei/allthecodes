//! Runtime adapter slots installed by the binary composition layer.

use std::sync::{Arc, OnceLock, RwLock};

use allthecodes_commands::Command;
use allthecodes_engine::command_runtime::CommandExecutor;
use allthecodes_engine::types::tool::Tools;
use allthecodes_types::commands::CommandDispatcher;
use anyhow::{Context, Result};
use serde_json::Value;

type InitPlugins = fn();
type ActiveTools = fn() -> Tools;
type Commands = fn() -> Vec<Command>;
type CommandDispatcherFactory = fn() -> Arc<dyn CommandDispatcher>;
type CommandExecutorFactory = fn() -> Arc<dyn CommandExecutor>;
type GithubPrActivityRouter =
    fn(&Value, Option<&str>, Option<&str>) -> Result<Option<GithubPrActivityRouteOutcome>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GithubPrActivityRouteOutcome {
    pub matched: usize,
    pub delivered: usize,
}

#[derive(Clone, Copy)]
pub struct DaemonRuntimeAdapters {
    pub init_plugins: InitPlugins,
    pub active_tools: ActiveTools,
    pub commands: Commands,
    pub command_dispatcher: CommandDispatcherFactory,
    pub command_executor: CommandExecutorFactory,
    pub route_github_pr_activity: GithubPrActivityRouter,
}

static ADAPTERS: OnceLock<RwLock<Option<DaemonRuntimeAdapters>>> = OnceLock::new();

pub fn set_runtime_adapters(adapters: DaemonRuntimeAdapters) {
    let slot = ADAPTERS.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(adapters);
    }
}

fn adapters() -> Result<DaemonRuntimeAdapters> {
    ADAPTERS
        .get()
        .and_then(|slot| slot.read().ok().and_then(|guard| *guard))
        .context("daemon runtime adapters are not installed")
}

pub(crate) fn init_plugins() -> Result<()> {
    (adapters()?.init_plugins)();
    Ok(())
}

pub(crate) fn active_tools() -> Result<Tools> {
    Ok((adapters()?.active_tools)())
}

pub(crate) fn commands() -> Result<Vec<Command>> {
    Ok((adapters()?.commands)())
}

pub(crate) fn command_names() -> Result<Vec<String>> {
    Ok(commands()?
        .iter()
        .map(|command| command.name.clone())
        .collect())
}

pub(crate) fn command_dispatcher() -> Result<Arc<dyn CommandDispatcher>> {
    Ok((adapters()?.command_dispatcher)())
}

pub(crate) fn command_executor() -> Result<Arc<dyn CommandExecutor>> {
    Ok((adapters()?.command_executor)())
}

pub(crate) fn route_github_pr_activity(
    payload: &Value,
    event: Option<&str>,
    delivery_id: Option<&str>,
) -> Result<Option<GithubPrActivityRouteOutcome>> {
    (adapters()?.route_github_pr_activity)(payload, event, delivery_id)
}
