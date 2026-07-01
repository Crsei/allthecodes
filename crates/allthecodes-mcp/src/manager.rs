//! McpManager -- manages multiple MCP server connections.
//!
//! Provides a high-level interface for discovering and connecting to MCP
//! servers, and aggregating their tools and resources.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

use super::client::McpClient;
use super::{
    McpResource, McpResourceWithServer, McpRuntimeContext, McpServerConfig, McpSubsystemEvent,
    McpToolDef, ReadResourceResult, SharedMcpEventSink,
};

const CONNECT_RETRY_ATTEMPTS: usize = 3;
const CONNECT_RETRY_BASE_DELAY_MS: u64 = 50;
const CONNECT_RETRY_MAX_DELAY_MS: u64 = 250;

/// Manages multiple MCP server connections.
pub struct McpManager {
    /// Active clients, keyed by server name.
    pub clients: HashMap<String, McpClient>,
    runtime: McpRuntimeContext,
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

    /// Connect to all configured MCP servers.
    ///
    /// Discovers servers from settings, connects to each one, and
    /// initializes them. Failures for individual servers are logged
    /// but do not prevent other servers from connecting.
    pub async fn connect_all(&mut self, configs: Vec<McpServerConfig>) -> Result<()> {
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
        &self,
        config: McpServerConfig,
    ) -> Result<McpClient> {
        let mut last_error = None;
        for attempt in 0..CONNECT_RETRY_ATTEMPTS {
            match self.connect_ready_client(config.clone()).await {
                Ok(client) => return Ok(client),
                Err(err) => {
                    let final_attempt = attempt + 1 >= CONNECT_RETRY_ATTEMPTS;
                    warn!(
                        server = %config.name,
                        attempt = attempt + 1,
                        max_attempts = CONNECT_RETRY_ATTEMPTS,
                        error = %err,
                        "MCP: connect attempt failed"
                    );
                    if final_attempt {
                        return Err(err);
                    }
                    last_error = Some(err);
                    sleep(Duration::from_millis(connect_retry_delay_ms(attempt))).await;
                }
            }
        }

        Err(last_error.expect("retry loop should have returned on final attempt"))
    }

    async fn connect_ready_client(&self, config: McpServerConfig) -> Result<McpClient> {
        let name = config.name.clone();
        let mut client = McpClient::with_runtime(config, self.runtime.clone());

        client.connect().await?;

        if let Err(e) = client.initialize().await {
            let error = e.to_string();
            warn!(
                server = %name,
                error = %error,
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

    /// Get all resources from all connected servers.
    pub fn all_resources(&self) -> Vec<McpResource> {
        self.clients
            .values()
            .flat_map(|c| c.resources.iter().cloned())
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
            true
        } else {
            false
        }
    }
}

pub(crate) fn connect_retry_delay_ms(attempt: usize) -> u64 {
    let factor = 1_u64 << attempt.min(8);
    CONNECT_RETRY_BASE_DELAY_MS
        .saturating_mul(factor)
        .min(CONNECT_RETRY_MAX_DELAY_MS)
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
}
