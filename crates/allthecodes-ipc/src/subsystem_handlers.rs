//! Runtime IPC facade for subsystem handlers.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use allthecodes_ipc_protocol::protocol::BackendMessage;
use allthecodes_ipc_protocol::subsystem_events::{
    IdeCommand, LspCommand, McpCommand, PluginCommand, SkillCommand,
};
use allthecodes_ipc_protocol::subsystem_types::*;

pub type BoxRuntimeFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpRuntimeOperation {
    Connect,
    Disconnect,
    Reconnect,
}

#[derive(Debug, Clone)]
pub struct McpRuntimeReport {
    pub server_name: String,
    pub state: String,
    pub error: Option<String>,
    pub text: String,
    pub level: String,
}

pub trait SubsystemRuntimeHost: Send + Sync + 'static {
    fn handle_lsp_command(&self, cmd: LspCommand) -> Vec<BackendMessage>;
    fn handle_mcp_command(&self, cmd: McpCommand) -> Vec<BackendMessage>;
    fn handle_mcp_command_with_runtime<'a>(
        &'a self,
        cmd: McpCommand,
        cwd: &'a Path,
    ) -> BoxRuntimeFuture<'a, Vec<BackendMessage>>;
    fn handle_plugin_command(&self, cmd: PluginCommand) -> Vec<BackendMessage>;
    fn handle_skill_command(&self, cmd: SkillCommand) -> Vec<BackendMessage>;
    fn handle_ide_command(&self, cmd: IdeCommand) -> Vec<BackendMessage>;
    fn build_subsystem_status_snapshot(&self) -> SubsystemStatusSnapshot;
    fn build_lsp_server_info_list(&self) -> Vec<LspServerInfo>;
    fn load_lsp_recommendation_settings(&self) -> LspRecommendationSettings;
    fn build_mcp_server_info_list(&self) -> Vec<McpServerStatusInfo>;
    fn build_mcp_server_info_list_for_cwd_async<'a>(
        &'a self,
        cwd: &'a Path,
    ) -> BoxRuntimeFuture<'a, Vec<McpServerStatusInfo>>;
    fn build_mcp_server_config_entries(&self, cwd: &Path) -> Vec<McpServerConfigEntry>;
    fn run_mcp_runtime_operation<'a>(
        &'a self,
        cwd: &'a Path,
        operation: McpRuntimeOperation,
        server_name: &'a str,
    ) -> BoxRuntimeFuture<'a, McpRuntimeReport>;
    fn build_plugin_info_list(&self) -> Vec<PluginInfo>;
    fn build_skill_info_list(&self) -> Vec<SkillInfo>;
    fn build_ide_info_list(&self) -> Vec<IdeInfo>;
}

static HOST: OnceLock<Arc<dyn SubsystemRuntimeHost>> = OnceLock::new();

pub fn set_runtime_host(host: Arc<dyn SubsystemRuntimeHost>) {
    let _ = HOST.set(host);
}

fn host() -> Option<&'static Arc<dyn SubsystemRuntimeHost>> {
    HOST.get()
}

pub fn handle_lsp_command(cmd: LspCommand) -> Vec<BackendMessage> {
    host()
        .map(|host| host.handle_lsp_command(cmd))
        .unwrap_or_default()
}

pub fn handle_mcp_command(cmd: McpCommand) -> Vec<BackendMessage> {
    host()
        .map(|host| host.handle_mcp_command(cmd))
        .unwrap_or_default()
}

pub async fn handle_mcp_command_with_runtime(cmd: McpCommand, cwd: &Path) -> Vec<BackendMessage> {
    match host() {
        Some(host) => host.handle_mcp_command_with_runtime(cmd, cwd).await,
        None => Vec::new(),
    }
}

pub fn handle_plugin_command(cmd: PluginCommand) -> Vec<BackendMessage> {
    host()
        .map(|host| host.handle_plugin_command(cmd))
        .unwrap_or_default()
}

pub fn handle_skill_command(cmd: SkillCommand) -> Vec<BackendMessage> {
    host()
        .map(|host| host.handle_skill_command(cmd))
        .unwrap_or_default()
}

pub fn handle_ide_command(cmd: IdeCommand) -> Vec<BackendMessage> {
    host()
        .map(|host| host.handle_ide_command(cmd))
        .unwrap_or_default()
}

pub fn build_subsystem_status_snapshot() -> SubsystemStatusSnapshot {
    host()
        .map(|host| host.build_subsystem_status_snapshot())
        .unwrap_or_else(empty_subsystem_status_snapshot)
}

fn empty_subsystem_status_snapshot() -> SubsystemStatusSnapshot {
    SubsystemStatusSnapshot {
        lsp: Vec::new(),
        mcp: Vec::new(),
        plugins: Vec::new(),
        skills: Vec::new(),
        ides: Vec::new(),
        timestamp: chrono::Utc::now().timestamp(),
    }
}

pub fn build_lsp_server_info_list() -> Vec<LspServerInfo> {
    host()
        .map(|host| host.build_lsp_server_info_list())
        .unwrap_or_default()
}

pub fn load_lsp_recommendation_settings() -> LspRecommendationSettings {
    host()
        .map(|host| host.load_lsp_recommendation_settings())
        .unwrap_or_default()
}

pub fn build_mcp_server_info_list() -> Vec<McpServerStatusInfo> {
    host()
        .map(|host| host.build_mcp_server_info_list())
        .unwrap_or_default()
}

pub async fn build_mcp_server_info_list_for_cwd_async(cwd: &Path) -> Vec<McpServerStatusInfo> {
    match host() {
        Some(host) => host.build_mcp_server_info_list_for_cwd_async(cwd).await,
        None => Vec::new(),
    }
}

pub fn build_mcp_server_config_entries(cwd: &Path) -> Vec<McpServerConfigEntry> {
    host()
        .map(|host| host.build_mcp_server_config_entries(cwd))
        .unwrap_or_default()
}

pub async fn run_mcp_runtime_operation(
    cwd: &Path,
    operation: McpRuntimeOperation,
    server_name: &str,
) -> McpRuntimeReport {
    match host() {
        Some(host) => {
            host.run_mcp_runtime_operation(cwd, operation, server_name)
                .await
        }
        None => McpRuntimeReport {
            server_name: server_name.to_string(),
            state: "error".to_string(),
            error: Some("IPC subsystem runtime host is not installed".to_string()),
            text: format!(
                "Cannot run MCP operation for `{}` because the IPC subsystem runtime host is not installed.",
                server_name
            ),
            level: "error".to_string(),
        },
    }
}

pub fn build_plugin_info_list() -> Vec<PluginInfo> {
    host()
        .map(|host| host.build_plugin_info_list())
        .unwrap_or_default()
}

pub fn build_skill_info_list() -> Vec<SkillInfo> {
    host()
        .map(|host| host.build_skill_info_list())
        .unwrap_or_default()
}

pub fn build_ide_info_list() -> Vec<IdeInfo> {
    host()
        .map(|host| host.build_ide_info_list())
        .unwrap_or_default()
}

pub fn current_dir_mcp_command(cmd: McpCommand) -> Vec<BackendMessage> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    handle_mcp_command_at_cwd(cmd, &cwd)
}

fn handle_mcp_command_at_cwd(cmd: McpCommand, _cwd: &Path) -> Vec<BackendMessage> {
    handle_mcp_command(cmd)
}
