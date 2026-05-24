//! Fast-path handlers that short-circuit full initialization.
//!
//! These run before Phase B so we can bypass tool registration, MCP
//! discovery, settings layering, etc. when the user only wants a quick
//! one-shot (e.g. `--version`, `--dump-system-prompt`) or a
//! special-purpose bridge (`--chrome-native-host`,
//! `--claude-in-chrome-mcp`).

use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use crate::runtime_config::{chrome_requested, resolve_cwd};
use allthecodes_engine::types::tool::Tool;

use crate::runtime_config::StartupCli;

pub trait DumpSystemPromptCli: StartupCli {
    fn model(&self) -> Option<&str>;
    fn system_prompt(&self) -> Option<&str>;
    fn append_system_prompt(&self) -> Option<&str>;
}

#[derive(Debug, Clone)]
pub struct SnapshotExportReport {
    pub output_dir: PathBuf,
    pub index_path: PathBuf,
    pub snapshot_count: usize,
}

/// Run the Chrome native-messaging host bridge. Does NOT set up tracing or
/// register tools — Chrome captures stderr as error logs, so we skip every
/// side-effect beyond bridging stdin↔socket.
pub fn run_chrome_native_host() -> ExitCode {
    let rt = tokio::runtime::Runtime::new().expect("create tokio runtime");
    rt.block_on(async {
        match allthecodes_browser::native_host::run().await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("chrome-native-host error: {e:#}");
                ExitCode::FAILURE
            }
        }
    })
}

/// Run the Claude-in-Chrome stdio MCP bridge. Spawned as an MCP subprocess
/// by the allthecodes MCP manager when `--chrome` is active.
pub fn run_claude_in_chrome_mcp() -> ExitCode {
    let rt = tokio::runtime::Runtime::new().expect("create tokio runtime");
    rt.block_on(async {
        match allthecodes_browser::mcp_bridge::run().await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("claude-in-chrome-mcp error: {e:#}");
                ExitCode::FAILURE
            }
        }
    })
}

/// Export accepted Rust TUI snapshots into a single human-review folder.
pub fn run_export_ui_snapshots(
    output_dir: &Path,
    export: impl FnOnce(&Path) -> anyhow::Result<SnapshotExportReport>,
) -> ExitCode {
    match export(output_dir) {
        Ok(report) => {
            println!(
                "exported {} Rust TUI snapshots to {}",
                report.snapshot_count,
                report.output_dir.display()
            );
            println!("index: {}", report.index_path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("export-ui-snapshots error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Print the resolved system prompt and exit. Populates the minimum state
/// required for the prompt builder (tools, MCP/browser detection, merged
/// language/style) without running the full Phase B pipeline.
pub fn run_dump_system_prompt(cli: &impl DumpSystemPromptCli, tools: &[Arc<dyn Tool>]) -> ExitCode {
    allthecodes_plugins::init_plugins();
    let cwd = resolve_cwd(cli);
    let cwd_path = std::path::Path::new(&cwd);

    let dump_settings = match allthecodes_config::settings::load_effective(cwd_path) {
        Ok(mut loaded) => {
            if let Err(e) =
                allthecodes_config::settings::apply_startup_runtime_env(&loaded.effective.env)
            {
                eprintln!("settings.env error: {e:#}");
                return ExitCode::FAILURE;
            }
            allthecodes_config::settings::refresh_process_env_overrides(&mut loaded);
            Some(loaded.effective)
        }
        Err(e) => {
            eprintln!("settings error: {e:#}");
            return ExitCode::FAILURE;
        }
    };

    let provider_default = allthecodes_api::api::client::ApiClient::from_env().and_then(|client| {
        matches!(
            client.config().provider,
            allthecodes_api::api::client::ApiProvider::Anthropic { .. }
        )
        .then(|| client.config().default_model.clone())
    });
    let model_owned = cli
        .model()
        .map(str::to_string)
        .or(provider_default)
        .unwrap_or_else(allthecodes_models::default_model_id);
    let model = model_owned.as_str();

    // Populate the browser MCP server registry from config alone (no live
    // connection). Config-flagged servers (`"browserMcp": true`) are
    // authoritative; the heuristic half would need connected tools and
    // isn't exercised here; use `--init-only` for that path.
    let server_configs = match discover_mcp_servers_for_fast_path(cwd_path) {
        Ok(configs) => configs,
        Err(e) => {
            eprintln!("MCP discovery error: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    let mut browser_servers = detect_browser_servers(&server_configs, tools);
    // Mirror the full-init path: when Chrome subsystem is requested via
    // CLI / env, pre-register the first-party server name so the
    // `# Browser Automation` prompt fires under --dump-system-prompt too.
    let chrome_config_default = dump_settings
        .as_ref()
        .and_then(|cfg| cfg.claude_in_chrome_default_enabled);
    if chrome_requested(cli, chrome_config_default) {
        browser_servers
            .insert(allthecodes_browser::common::CLAUDE_IN_CHROME_MCP_SERVER_NAME.to_string());
    }
    allthecodes_browser::detection::install_browser_servers(browser_servers);

    let dump_lang = dump_settings.as_ref().and_then(|s| s.language.clone());
    let dump_style = dump_settings.as_ref().and_then(|s| s.output_style.clone());
    let include_auto_memory = dump_settings
        .as_ref()
        .and_then(|s| s.auto_memory_enabled)
        .unwrap_or(false);
    let session_memory_context = {
        let mut service = allthecodes_services::session_memory::SessionMemoryService::new(
            allthecodes_services::session_memory::SessionMemoryConfig::default(),
        );
        match service.load_from_disk() {
            Ok(()) => service.format_memory_context_for_workspace(5, Some(cwd_path)),
            Err(e) => {
                tracing::debug!(
                    error = %e,
                    "failed to load session memory for dump-system-prompt"
                );
                None
            }
        }
    };
    let (parts, _, _) = allthecodes_engine::system_prompt::build_system_prompt_with_session_memory(
        cli.system_prompt(),
        cli.append_system_prompt(),
        tools,
        model,
        &cwd,
        dump_lang.as_deref(),
        dump_style.as_deref(),
        include_auto_memory,
        session_memory_context.as_deref(),
    );
    for part in &parts {
        println!("{}", part);
    }
    ExitCode::SUCCESS
}

fn detect_browser_servers(
    configs: &[allthecodes_mcp::McpServerConfig],
    tools: &[Arc<dyn Tool>],
) -> std::collections::HashSet<String> {
    let mut servers: std::collections::HashSet<String> = configs
        .iter()
        .filter(|config| config.browser_mcp.unwrap_or(false))
        .map(|config| config.name.clone())
        .collect();

    for tool in tools {
        let name = tool.user_facing_name(None);
        if let Some((server, _)) = allthecodes_browser::detection::extract_browser_action(&name) {
            servers.insert(server.to_string());
        }
    }

    servers
}

fn discover_mcp_servers_for_fast_path(
    cwd_path: &Path,
) -> anyhow::Result<Vec<allthecodes_mcp::McpServerConfig>> {
    allthecodes_mcp::discovery::discover_mcp_servers(cwd_path)
}

#[cfg(test)]
mod tests {
    use super::discover_mcp_servers_for_fast_path;
    use serial_test::serial;
    use tempfile::TempDir;

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

    #[test]
    #[serial]
    fn fast_path_missing_mcp_settings_remains_empty() {
        let cc_rust_home = TempDir::new().expect("cc_rust_home tempdir");
        let cwd = TempDir::new().expect("cwd tempdir");
        let _home = EnvGuard::set(
            "CC_RUST_HOME",
            cc_rust_home.path().to_str().expect("utf8 tempdir"),
        );

        let configs =
            discover_mcp_servers_for_fast_path(cwd.path()).expect("missing settings is allowed");
        assert!(configs.is_empty());
    }

    #[test]
    #[serial]
    fn fast_path_existing_invalid_mcp_settings_returns_diagnostic() {
        let cc_rust_home = TempDir::new().expect("cc_rust_home tempdir");
        let cwd = TempDir::new().expect("cwd tempdir");
        let _home = EnvGuard::set(
            "CC_RUST_HOME",
            cc_rust_home.path().to_str().expect("utf8 tempdir"),
        );

        std::fs::write(cc_rust_home.path().join("settings.json"), "{not-json")
            .expect("write malformed settings");

        let err = discover_mcp_servers_for_fast_path(cwd.path())
            .expect_err("existing invalid settings must be diagnostic");
        assert!(err.to_string().contains("failed to parse MCP settings"));
    }
}
