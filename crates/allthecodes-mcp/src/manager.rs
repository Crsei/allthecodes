//! McpManager -- manages multiple MCP server connections.
//!
//! Provides a high-level interface for discovering and connecting to MCP
//! servers, and aggregating their tools and resources.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use allthecodes_tools::runtime_capability::{RuntimeCapability, RuntimeCapabilityRegistry};
use allthecodes_types::mcp::{McpBinding, McpBindingContext, McpPermission, McpToolScope};
use anyhow::Result;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

use super::client::McpClient;
use super::discovery::BoundMcpServerConfig;
use super::{
    McpResource, McpResourceWithServer, McpRuntimeContext, McpServerConfig,
    McpServerHealthSnapshot, McpSubsystemEvent, McpToolDef, ReadResourceResult, SharedMcpEventSink,
};

const CONNECT_RETRY_ATTEMPTS: usize = 3;
const CONNECT_RETRY_BASE_DELAY_MS: u64 = 50;
const CONNECT_RETRY_MAX_DELAY_MS: u64 = 250;

/// Manages multiple MCP server connections.
pub struct McpManager {
    /// Active clients, keyed by server name.
    pub clients: HashMap<String, McpClient>,
    runtime: McpRuntimeContext,
    bindings: Vec<McpBinding>,
    display_names: HashMap<String, String>,
    source_scopes: HashMap<String, String>,
    health: HashMap<String, McpServerHealthSnapshot>,
}

impl McpManager {
    pub fn new() -> Self {
        Self::with_runtime(McpRuntimeContext::new())
    }

    pub fn with_event_sink(event_sink: Arc<dyn super::McpEventSink>) -> Self {
        Self::with_runtime(McpRuntimeContext::with_event_sink(event_sink))
    }

    pub fn with_runtime(runtime: McpRuntimeContext) -> Self {
        Self {
            clients: HashMap::new(),
            runtime,
            bindings: Vec::new(),
            display_names: HashMap::new(),
            source_scopes: HashMap::new(),
            health: HashMap::new(),
        }
    }

    pub fn set_event_sink(&mut self, event_sink: Option<SharedMcpEventSink>) {
        self.runtime.set_event_sink(event_sink.clone());
        for client in self.clients.values_mut() {
            client.set_event_sink(event_sink.clone());
        }
    }

    pub fn emit_event(&self, event: McpSubsystemEvent) {
        self.runtime.emit_event(event);
    }

    pub fn set_bindings(&mut self, bindings: Vec<McpBinding>) {
        self.bindings = bindings;
        self.runtime.emit_event(McpSubsystemEvent::BindingsUpdated {
            bindings: self.bindings.clone(),
        });
    }

    pub fn bindings(&self) -> Vec<McpBinding> {
        self.bindings.clone()
    }

    pub fn server_display_name<'a>(&'a self, server_id: &'a str) -> &'a str {
        self.display_names
            .get(server_id)
            .map(String::as_str)
            .unwrap_or(server_id)
    }

    pub fn server_source_scope(&self, server_id: &str) -> Option<&str> {
        self.source_scopes.get(server_id).map(String::as_str)
    }

    pub fn health_snapshots(&self) -> Vec<McpServerHealthSnapshot> {
        let mut snapshots = self.health.values().cloned().collect::<Vec<_>>();
        snapshots.sort_by(|left, right| left.server_name.cmp(&right.server_name));
        snapshots
    }

    /// Connect to all configured MCP servers.
    ///
    /// Discovers servers from settings, connects to each one, and
    /// initializes them. Failures for individual servers are logged
    /// but do not prevent other servers from connecting.
    pub async fn connect_all(&mut self, configs: Vec<McpServerConfig>) -> Result<()> {
        if self.bindings.is_empty() {
            self.bindings = configs
                .iter()
                .map(|config| McpBinding {
                    server_id: config.name.clone(),
                    scope: McpToolScope::Global,
                    permissions: McpBinding::full_permissions(),
                    ..Default::default()
                })
                .collect();
        }
        for config in configs {
            let name = config.name.clone();
            if let Err(e) = self.connect_server(config).await {
                warn!(
                    server = %name,
                    error = %e,
                    "MCP: failed to connect to server"
                );
            }
        }

        Ok(())
    }

    pub async fn connect_all_bound(
        &mut self,
        configs: Vec<BoundMcpServerConfig>,
        bindings: Vec<McpBinding>,
    ) -> Result<()> {
        self.set_bindings(bindings);
        for bound in configs {
            let name = bound.server_id.clone();
            if let Err(e) = self.connect_bound_server(bound).await {
                warn!(
                    server = %name,
                    error = %e,
                    "MCP: failed to connect to bound server"
                );
            }
        }
        Ok(())
    }

    pub async fn connect_bound_server(&mut self, bound: BoundMcpServerConfig) -> Result<()> {
        let mut config = bound.config;
        config.name = bound.server_id.clone();
        self.display_names
            .insert(bound.server_id.clone(), bound.display_name);
        self.source_scopes
            .insert(bound.server_id.clone(), bound.source_scope);
        self.connect_server(config).await
    }

    /// Connect a single configured server and replace any existing client with
    /// the same name.
    ///
    /// Disabled configs remove any live client and emit a `disabled` state.
    /// Failed connection or initialization attempts leave no stale client in
    /// the manager.
    pub async fn connect_server(&mut self, config: McpServerConfig) -> Result<()> {
        let name = config.name.clone();
        self.disconnect_server(&name).await;

        // Respect the soft-disable flag from settings. Keeping the entry out
        // of `self.clients` means `list_tools`, `all_tools`, etc. behave as if
        // the server does not exist for this session, while the on-disk config
        // is preserved for a later re-enable.
        if config.disabled.unwrap_or(false) {
            tracing::info!(server = %name, "MCP: server disabled in settings, skipping");
            self.record_health_disabled(&config);
            self.runtime
                .emit_event(super::McpSubsystemEvent::ServerStateChanged {
                    server_name: name,
                    state: "disabled".to_string(),
                    error: None,
                });
            return Ok(());
        }

        let client = self.connect_ready_client_with_retries(config).await?;
        self.clients.insert(name, client);
        Ok(())
    }

    /// Reconnect a single server by dropping any current client before trying
    /// the new configuration.
    pub async fn reconnect_server(&mut self, config: McpServerConfig) -> Result<()> {
        let name = config.name.clone();
        self.disconnect_server(&name).await;
        self.runtime
            .emit_event(super::McpSubsystemEvent::ServerStateChanged {
                server_name: name,
                state: "pending".to_string(),
                error: None,
            });
        self.connect_server(config).await
    }

    async fn connect_ready_client_with_retries(
        &mut self,
        config: McpServerConfig,
    ) -> Result<McpClient> {
        let mut last_error = None;
        for attempt in 0..CONNECT_RETRY_ATTEMPTS {
            self.record_health_attempt(&config, attempt + 1, None);
            match self.connect_ready_client(config.clone()).await {
                Ok(client) => {
                    let recovered = self.record_health_success(&config, &client);
                    if recovered {
                        info!(
                            event = "McpServerRecovered",
                            server = %config.name,
                            attempt = attempt + 1,
                            max_attempts = CONNECT_RETRY_ATTEMPTS,
                            "MCP: server recovered after connection failure"
                        );
                    }
                    return Ok(client);
                }
                Err(err) => {
                    let final_attempt = attempt + 1 >= CONNECT_RETRY_ATTEMPTS;
                    let delay_ms = connect_retry_delay_ms(attempt);
                    let next_retry_at =
                        (!final_attempt).then(|| now_millis().saturating_add(delay_ms as i64));
                    self.record_health_failure(&config, attempt + 1, &err, next_retry_at);
                    let error_kind = classify_mcp_connect_error(&err);
                    if final_attempt {
                        warn!(
                            event = "McpServerRetryExhausted",
                            server = %config.name,
                            attempt = attempt + 1,
                            max_attempts = CONNECT_RETRY_ATTEMPTS,
                            error_kind,
                            error = %err,
                            "MCP: connect retry exhausted"
                        );
                        return Err(err);
                    }
                    warn!(
                        event = "McpServerRetryScheduled",
                        server = %config.name,
                        attempt = attempt + 1,
                        max_attempts = CONNECT_RETRY_ATTEMPTS,
                        delay_ms,
                        next_retry_at = ?next_retry_at,
                        error_kind,
                        error = %err,
                        "MCP: connect retry scheduled"
                    );
                    last_error = Some(err);
                    sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }

        Err(last_error.expect("retry loop should have returned on final attempt"))
    }

    async fn connect_ready_client(&self, config: McpServerConfig) -> Result<McpClient> {
        let name = config.name.clone();
        let mut client = McpClient::with_runtime(config, self.runtime.clone());

        client.connect().await?;

        let initialize_started_at = now_millis();
        if let Err(e) = client.initialize().await {
            let error = e.to_string();
            let initialize_latency_ms = now_millis().saturating_sub(initialize_started_at);
            warn!(
                server = %name,
                error = %error,
                initialize_latency_ms,
                "MCP: failed to initialize server"
            );
            self.runtime
                .emit_event(super::McpSubsystemEvent::ServerStateChanged {
                    server_name: name,
                    state: "error".to_string(),
                    error: Some(error),
                });
            client.disconnect().await;
            return Err(e);
        }

        // Discover tools if supported
        if client.supports_tools() {
            if let Err(e) = client.list_tools().await {
                warn!(
                    server = %name,
                    error = %e,
                    "MCP: failed to list tools"
                );
            }
        }

        // Discover resources if supported
        if client.supports_resources() {
            if let Err(e) = client.list_resources().await {
                warn!(
                    server = %name,
                    error = %e,
                    "MCP: failed to list resources"
                );
            }
        }

        info!(
            server = %name,
            tools = client.tools.len(),
            resources = client.resources.len(),
            initialize_latency_ms = now_millis().saturating_sub(initialize_started_at),
            "MCP: server ready"
        );

        Ok(client)
    }

    /// Get all tools from all connected servers.
    pub fn all_tools(&self) -> Vec<McpToolDef> {
        self.clients
            .values()
            .flat_map(|c| c.tools.iter().cloned())
            .collect()
    }

    pub fn runtime_capabilities(&self) -> RuntimeCapabilityRegistry {
        let mut registry = RuntimeCapabilityRegistry::new();
        for (server_id, client) in &self.clients {
            for tool in &client.tools {
                let server_name = if tool.server_name.is_empty() {
                    server_id.as_str()
                } else {
                    tool.server_name.as_str()
                };
                let mut capability = RuntimeCapability::mcp_tool(server_name, &tool.name);
                if !tool.description.is_empty() {
                    capability.description = Some(tool.description.clone());
                }
                registry.register(capability);
            }
        }
        registry
    }

    pub fn tools_for_context(&self, ctx: &McpBindingContext) -> Vec<McpToolDef> {
        self.clients
            .iter()
            .filter(|(server_id, _)| self.server_allowed(ctx, server_id, McpPermission::ListTools))
            .flat_map(|(_, client)| client.tools.iter().cloned())
            .collect()
    }

    /// Get all resources from all connected servers.
    pub fn all_resources(&self) -> Vec<McpResource> {
        self.clients
            .values()
            .flat_map(|c| c.resources.iter().cloned())
            .collect()
    }

    pub fn resources_for_context(&self, ctx: &McpBindingContext) -> Vec<McpResource> {
        self.clients
            .iter()
            .filter(|(server_id, _)| {
                self.server_allowed(ctx, server_id, McpPermission::ReadResources)
            })
            .flat_map(|(_, client)| client.resources.iter().cloned())
            .collect()
    }

    /// List resources from all connected servers, optionally restricted to one
    /// server. Each result includes the owning server so callers can pass it
    /// directly to `read_resource`.
    pub fn list_resources(&self, server: Option<&str>) -> Result<Vec<McpResourceWithServer>> {
        let clients = self.clients_for_resource_query(server)?;
        Ok(clients
            .into_iter()
            .flat_map(|(server_name, client)| {
                client
                    .resources
                    .iter()
                    .cloned()
                    .map(move |resource| McpResourceWithServer {
                        server: server_name.clone(),
                        uri: resource.uri,
                        name: resource.name,
                        description: resource.description,
                        mime_type: resource.mime_type,
                    })
            })
            .collect())
    }

    pub fn list_resources_for_context(
        &self,
        ctx: &McpBindingContext,
        server: Option<&str>,
    ) -> Result<Vec<McpResourceWithServer>> {
        let clients = self.clients_for_resource_query(server)?;
        Ok(clients
            .into_iter()
            .filter(|(server_name, _)| {
                self.server_allowed(ctx, server_name, McpPermission::ReadResources)
            })
            .flat_map(|(server_name, client)| {
                client
                    .resources
                    .iter()
                    .cloned()
                    .map(move |resource| McpResourceWithServer {
                        server: server_name.clone(),
                        uri: resource.uri,
                        name: resource.name,
                        description: resource.description,
                        mime_type: resource.mime_type,
                    })
            })
            .collect())
    }

    /// Read a concrete MCP resource from a named connected server.
    pub async fn read_resource(&self, server: &str, uri: &str) -> Result<ReadResourceResult> {
        let client = self.clients.get(server).ok_or_else(|| {
            anyhow::anyhow!(
                "MCP server '{}' not found. Available servers: {}",
                server,
                self.available_server_list()
            )
        })?;

        if !client.supports_resources() {
            anyhow::bail!("MCP server '{}' does not support resources", server);
        }

        client.read_resource(uri).await
    }

    pub async fn read_resource_for_context(
        &self,
        ctx: &McpBindingContext,
        server: &str,
        uri: &str,
    ) -> Result<ReadResourceResult> {
        if !self.server_allowed(ctx, server, McpPermission::ReadResources) {
            anyhow::bail!(
                "MCP resource read denied: server '{}' is not bound with read_resources permission for thread '{}'",
                server,
                ctx.thread_id
            );
        }
        self.read_resource(server, uri).await
    }

    fn clients_for_resource_query(
        &self,
        server: Option<&str>,
    ) -> Result<Vec<(String, &McpClient)>> {
        if let Some(server) = server {
            let client = self.clients.get(server).ok_or_else(|| {
                anyhow::anyhow!(
                    "MCP server '{}' not found. Available servers: {}",
                    server,
                    self.available_server_list()
                )
            })?;
            return Ok(vec![(server.to_string(), client)]);
        }

        Ok(self
            .clients
            .iter()
            .map(|(server_name, client)| (server_name.clone(), client))
            .collect())
    }

    fn available_server_list(&self) -> String {
        let mut names = self.clients.keys().cloned().collect::<Vec<_>>();
        names.sort();
        if names.is_empty() {
            "(none)".to_string()
        } else {
            names.join(", ")
        }
    }

    /// Find the client that owns a tool by name.
    pub fn find_client_for_tool(&self, tool_name: &str) -> Option<&McpClient> {
        self.clients
            .values()
            .find(|c| c.tools.iter().any(|t| t.name == tool_name))
    }

    pub fn can_call_tool(&self, ctx: &McpBindingContext, server_id: &str, tool: &str) -> bool {
        self.server_allowed(ctx, server_id, McpPermission::CallTools)
            && self
                .clients
                .get(server_id)
                .map(|client| client.tools.iter().any(|def| def.name == tool))
                .unwrap_or(false)
    }

    pub fn permission_denied_message(
        &self,
        ctx: &McpBindingContext,
        server_id: &str,
        permission: McpPermission,
    ) -> String {
        format!(
            "MCP permission denied: server '{}' is not bound with {} permission for thread '{}'",
            server_id,
            permission.as_str(),
            ctx.thread_id
        )
    }

    fn server_allowed(
        &self,
        ctx: &McpBindingContext,
        server_id: &str,
        permission: McpPermission,
    ) -> bool {
        if self.bindings.is_empty() {
            return true;
        }
        self.bindings.iter().any(|binding| {
            binding.server_id == server_id
                && ctx.matches_binding(binding)
                && binding.allows(McpPermission::Connect)
                && binding.allows(permission)
        })
    }

    /// Disconnect from all servers.
    pub async fn disconnect_all(&mut self) {
        let names: Vec<String> = self.clients.keys().cloned().collect();
        for name in names {
            self.disconnect_server(&name).await;
        }
    }

    /// Disconnect a single server. Returns `true` if a live client existed.
    pub async fn disconnect_server(&mut self, name: &str) -> bool {
        if let Some(mut client) = self.clients.remove(name) {
            client.disconnect().await;
            if let Some(snapshot) = self.health.get_mut(name) {
                snapshot.state = "disconnected".to_string();
                snapshot.next_retry_at = None;
            }
            true
        } else {
            false
        }
    }

    fn health_entry(&mut self, config: &McpServerConfig) -> &mut McpServerHealthSnapshot {
        self.health
            .entry(config.name.clone())
            .or_insert_with(|| McpServerHealthSnapshot::new(&config.name, &config.transport))
    }

    fn record_health_attempt(
        &mut self,
        config: &McpServerConfig,
        attempt: usize,
        next_retry_at: Option<i64>,
    ) {
        let now = now_millis();
        let snapshot = self.health_entry(config);
        snapshot.state = "connecting".to_string();
        snapshot.transport = config.transport.clone();
        snapshot.last_attempt_at = Some(now);
        snapshot.failure_count = attempt.saturating_sub(1) as u32;
        snapshot.connect_attempt_count = snapshot.connect_attempt_count.saturating_add(1);
        snapshot.next_retry_at = next_retry_at;
    }

    fn record_health_failure(
        &mut self,
        config: &McpServerConfig,
        attempt: usize,
        error: &anyhow::Error,
        next_retry_at: Option<i64>,
    ) {
        let (state, error_message) = {
            let snapshot = self.health_entry(config);
            let state = if next_retry_at.is_some() {
                snapshot.retry_scheduled_count = snapshot.retry_scheduled_count.saturating_add(1);
                "retrying"
            } else {
                snapshot.retry_exhausted_count = snapshot.retry_exhausted_count.saturating_add(1);
                "error"
            };
            snapshot.state = state.to_string();
            snapshot.last_error = Some(error.to_string());
            snapshot.last_error_kind = Some(classify_mcp_connect_error(error).to_string());
            snapshot.failure_count = attempt as u32;
            snapshot.next_retry_at = next_retry_at;
            snapshot.tools_count = None;
            snapshot.resources_count = None;
            (snapshot.state.clone(), error.to_string())
        };
        self.runtime
            .emit_event(super::McpSubsystemEvent::ServerStateChanged {
                server_name: config.name.clone(),
                state,
                error: Some(error_message),
            });
    }

    fn record_health_success(&mut self, config: &McpServerConfig, client: &McpClient) -> bool {
        let now = now_millis();
        let recovered = {
            let snapshot = self.health_entry(config);
            let recovered = snapshot.failure_count > 0 || snapshot.last_error.is_some();
            snapshot.state = "connected".to_string();
            snapshot.transport = config.transport.clone();
            snapshot.last_success_at = Some(now);
            snapshot.last_error = None;
            snapshot.last_error_kind = None;
            snapshot.failure_count = 0;
            if recovered {
                snapshot.recovered_count = snapshot.recovered_count.saturating_add(1);
            }
            snapshot.next_retry_at = None;
            snapshot.stderr_tail = client.stderr_tail();
            snapshot.stderr_tail_dropped_line_count = client.stderr_tail_dropped_line_count();
            snapshot.tools_count = Some(client.tools.len());
            snapshot.resources_count = Some(client.resources.len());
            recovered
        };
        self.runtime
            .emit_event(super::McpSubsystemEvent::ServerStateChanged {
                server_name: config.name.clone(),
                state: "connected".to_string(),
                error: None,
            });
        recovered
    }

    fn record_health_disabled(&mut self, config: &McpServerConfig) {
        let snapshot = self.health_entry(config);
        snapshot.state = "disabled".to_string();
        snapshot.transport = config.transport.clone();
        snapshot.next_retry_at = None;
        snapshot.last_error = None;
        snapshot.last_error_kind = None;
    }
}

pub(crate) fn connect_retry_delay_ms(attempt: usize) -> u64 {
    let factor = 1_u64 << attempt.min(8);
    CONNECT_RETRY_BASE_DELAY_MS
        .saturating_mul(factor)
        .min(CONNECT_RETRY_MAX_DELAY_MS)
}

fn classify_mcp_connect_error(error: &anyhow::Error) -> &'static str {
    let message = format!("{error:#}");
    if message.contains("failed to spawn MCP server") {
        "spawn_failed"
    } else if message.contains("timed out") && message.contains("initialize") {
        "initialize_timeout"
    } else if message.contains("failed to parse initialize response")
        || message.contains("JSON-RPC")
        || message.contains("protocol")
    {
        "protocol_parse_error"
    } else if super::client::is_auth_needed_error(error) {
        "auth_needed"
    } else {
        "connection_failed"
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{McpConnectionState, ServerCapabilities};

    fn test_client(name: &str, resources: Vec<McpResource>, supports_resources: bool) -> McpClient {
        let mut client = McpClient::new(McpServerConfig {
            name: name.to_string(),
            transport: "stdio".to_string(),
            command: Some("dummy".to_string()),
            args: None,
            url: None,
            headers: None,
            oauth: None,
            env: None,
            browser_mcp: None,
            disabled: None,
            bearer_token_env_var: None,
            env_http_headers: None,
            auth: None,
        });
        client.state = McpConnectionState::Connected;
        client.resources = resources;
        if supports_resources {
            client.server_capabilities = ServerCapabilities {
                resources: Some(json!({})),
                ..Default::default()
            };
        }
        client
    }

    fn tool_client(name: &str, tool_name: &str) -> McpClient {
        let mut client = test_client(name, Vec::new(), false);
        client.tools = vec![McpToolDef {
            name: tool_name.to_string(),
            description: String::new(),
            input_schema: json!({"type": "object"}),
            server_name: name.to_string(),
        }];
        client
    }

    #[test]
    fn list_resources_includes_server_owner_and_filters() {
        let mut manager = McpManager::new();
        manager.clients.insert(
            "alpha".to_string(),
            test_client(
                "alpha",
                vec![McpResource {
                    uri: "file:///alpha".to_string(),
                    name: "Alpha".to_string(),
                    description: Some("alpha resource".to_string()),
                    mime_type: Some("text/plain".to_string()),
                }],
                true,
            ),
        );
        manager.clients.insert(
            "beta".to_string(),
            test_client(
                "beta",
                vec![McpResource {
                    uri: "file:///beta".to_string(),
                    name: "Beta".to_string(),
                    description: None,
                    mime_type: None,
                }],
                true,
            ),
        );

        let all = manager.list_resources(None).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|resource| resource.server == "alpha"
            && resource.uri == "file:///alpha"
            && resource.mime_type.as_deref() == Some("text/plain")));

        let beta = manager.list_resources(Some("beta")).unwrap();
        assert_eq!(beta.len(), 1);
        assert_eq!(beta[0].server, "beta");
        assert_eq!(beta[0].uri, "file:///beta");
    }

    #[test]
    fn list_resources_reports_available_servers_for_missing_filter() {
        let mut manager = McpManager::new();
        manager
            .clients
            .insert("alpha".to_string(), test_client("alpha", Vec::new(), true));
        let err = manager.list_resources(Some("missing")).unwrap_err();
        assert!(err.to_string().contains("MCP server 'missing' not found"));
        assert!(err.to_string().contains("alpha"));
    }

    #[tokio::test]
    async fn read_resource_rejects_servers_without_resource_capability() {
        let mut manager = McpManager::new();
        manager
            .clients
            .insert("alpha".to_string(), test_client("alpha", Vec::new(), false));
        let err = manager
            .read_resource("alpha", "file:///alpha")
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("MCP server 'alpha' does not support resources"));
    }

    #[test]
    fn tools_for_context_filters_session_and_thread_bindings() {
        let mut manager = McpManager::new();
        manager
            .clients
            .insert("alpha".to_string(), tool_client("alpha", "search"));
        manager
            .clients
            .insert("beta".to_string(), tool_client("beta", "read"));
        manager.set_bindings(vec![
            McpBinding {
                server_id: "alpha".to_string(),
                scope: McpToolScope::Session,
                session_id: Some("s1".to_string()),
                permissions: McpBinding::full_permissions(),
                ..Default::default()
            },
            McpBinding {
                server_id: "beta".to_string(),
                scope: McpToolScope::Thread,
                session_id: Some("s1".to_string()),
                thread_id: Some("agent-a".to_string()),
                permissions: McpBinding::full_permissions(),
                ..Default::default()
            },
        ]);

        let main_ctx = McpBindingContext::main(None, "s1");
        let main_tools = manager.tools_for_context(&main_ctx);
        assert_eq!(main_tools.len(), 1);
        assert_eq!(main_tools[0].server_name, "alpha");

        let agent_ctx = McpBindingContext::agent(None, "s1", "agent-a");
        let agent_tools = manager.tools_for_context(&agent_ctx);
        assert_eq!(agent_tools.len(), 1);
        assert_eq!(agent_tools[0].server_name, "beta");

        let sibling_ctx = McpBindingContext::agent(None, "s1", "agent-b");
        assert!(manager.tools_for_context(&sibling_ctx).is_empty());
    }

    #[test]
    fn can_call_tool_requires_call_tools_permission() {
        let mut manager = McpManager::new();
        manager
            .clients
            .insert("alpha".to_string(), tool_client("alpha", "search"));
        manager.set_bindings(vec![McpBinding {
            server_id: "alpha".to_string(),
            scope: McpToolScope::Global,
            permissions: vec![McpPermission::Connect, McpPermission::ListTools],
            ..Default::default()
        }]);

        let ctx = McpBindingContext::main(None, "s1");
        assert!(!manager.can_call_tool(&ctx, "alpha", "search"));
        assert!(manager
            .permission_denied_message(&ctx, "alpha", McpPermission::CallTools)
            .contains("call_tools"));
    }

    #[test]
    fn runtime_capabilities_project_mcp_tools_with_permission_subject() {
        let mut manager = McpManager::new();
        manager.clients.insert(
            "alpha".to_string(),
            tool_client("alpha", "search_repository"),
        );

        let registry = manager.runtime_capabilities();
        let capability = registry
            .get("alpha/search_repository")
            .expect("mcp tool capability");

        assert_eq!(
            capability.kind,
            allthecodes_tools::runtime_capability::RuntimeCapabilityKind::McpServerTool
        );
        assert_eq!(
            capability.permission_subject.as_deref(),
            Some("McpTool(alpha/search_repository)")
        );
        assert_eq!(
            capability.source_scope,
            allthecodes_tools::runtime_capability::RuntimeCapabilitySourceScope::McpServer(
                "alpha".to_string()
            )
        );
    }

    #[tokio::test]
    async fn failed_stdio_spawn_records_health_snapshot() {
        let mut manager = McpManager::new();
        let config = McpServerConfig {
            name: "broken".to_string(),
            transport: "stdio".to_string(),
            command: Some("__allthecodes_missing_mcp_server__".to_string()),
            args: None,
            url: None,
            headers: None,
            oauth: None,
            env: None,
            browser_mcp: None,
            disabled: None,
            bearer_token_env_var: None,
            env_http_headers: None,
            auth: None,
        };

        let err = manager.connect_server(config).await.unwrap_err();
        assert!(err.to_string().contains("failed to spawn MCP server"));

        let health = manager.health_snapshots();
        let broken = health
            .iter()
            .find(|snapshot| snapshot.server_name == "broken")
            .expect("broken health snapshot");

        assert_eq!(broken.state, "error");
        assert_eq!(broken.transport, "stdio");
        assert_eq!(broken.last_error_kind.as_deref(), Some("spawn_failed"));
        assert_eq!(broken.failure_count, CONNECT_RETRY_ATTEMPTS as u32);
        assert_eq!(broken.connect_attempt_count, CONNECT_RETRY_ATTEMPTS as u64);
        assert_eq!(
            broken.retry_scheduled_count,
            CONNECT_RETRY_ATTEMPTS.saturating_sub(1) as u64
        );
        assert_eq!(broken.retry_exhausted_count, 1);
        assert_eq!(broken.recovered_count, 0);
        assert_eq!(broken.stderr_tail_dropped_line_count, 0);
        assert!(broken.last_attempt_at.is_some());
        assert!(broken.next_retry_at.is_none());
    }

    #[test]
    fn health_success_records_recovery_after_failure() {
        let mut manager = McpManager::new();
        let config = McpServerConfig {
            name: "recovering".to_string(),
            transport: "stdio".to_string(),
            command: Some("dummy".to_string()),
            args: None,
            url: None,
            headers: None,
            oauth: None,
            env: None,
            browser_mcp: None,
            disabled: None,
            bearer_token_env_var: None,
            env_http_headers: None,
            auth: None,
        };
        let error = anyhow::anyhow!("initialize timed out");

        manager.record_health_attempt(&config, 1, None);
        manager.record_health_failure(&config, 1, &error, None);
        let recovered =
            manager.record_health_success(&config, &tool_client("recovering", "search"));

        assert!(recovered);
        let snapshot = manager
            .health_snapshots()
            .into_iter()
            .find(|snapshot| snapshot.server_name == "recovering")
            .expect("recovering health snapshot");
        assert_eq!(snapshot.state, "connected");
        assert_eq!(snapshot.failure_count, 0);
        assert_eq!(snapshot.connect_attempt_count, 1);
        assert_eq!(snapshot.retry_scheduled_count, 0);
        assert_eq!(snapshot.retry_exhausted_count, 1);
        assert_eq!(snapshot.recovered_count, 1);
        assert_eq!(snapshot.stderr_tail_dropped_line_count, 0);
    }
}
