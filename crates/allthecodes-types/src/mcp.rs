use serde::{Deserialize, Serialize};

/// MCP protocol version used by stdio MCP peers.
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// Client name advertised by allthecodes MCP clients.
pub const CLIENT_NAME: &str = "allthecodes";

/// Client version advertised by allthecodes MCP clients.
pub const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default connection and initialization timeout in seconds.
pub const CONNECT_TIMEOUT_SECS: u64 = 30;

/// Default tool-call timeout in seconds.
pub const TOOL_CALL_TIMEOUT_SECS: u64 = 300;

/// OAuth configuration for an MCP server.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpOAuthConfig {
    /// Public OAuth client identifier. When omitted, allthecodes uses a stable
    /// default public-client id (`allthecodes`).
    #[serde(default, rename = "clientId", skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Loopback callback port to place in the OAuth redirect URI.
    #[serde(
        default,
        rename = "callbackPort",
        skip_serializing_if = "Option::is_none"
    )]
    pub callback_port: Option<u16>,
    /// Authorization-server metadata URL (RFC 8414). If absent, discovery tries
    /// the MCP resource metadata endpoint first and then the origin's default
    /// authorization-server metadata path.
    #[serde(
        default,
        rename = "authServerMetadataUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub auth_server_metadata_url: Option<String>,
    /// OAuth scopes to request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
    /// Optional OAuth resource parameter (RFC 8707) to include during
    /// authorization and token requests. When absent, the MCP server URL
    /// is used as the resource if available.
    #[serde(
        default,
        rename = "oauthResource",
        alias = "oauth_resource",
        skip_serializing_if = "Option::is_none"
    )]
    pub oauth_resource: Option<String>,
}
