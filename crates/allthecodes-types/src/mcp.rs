use std::path::{Path, PathBuf};

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
    /// Credential store backend selection.
    ///
    /// * `"auto"` (default): try system keyring first, fall back to file store.
    /// * `"file"`: use JSON file at `{data_root}/mcp-oauth.json` only.
    /// * `"keyring"`: use system keyring only.
    ///
    /// When `None` or absent, behaves as `"auto"`.
    #[serde(
        default,
        rename = "credentialsStore",
        alias = "credentials_store",
        skip_serializing_if = "Option::is_none"
    )]
    pub credentials_store: Option<String>,
}

/// Runtime scope where an MCP server binding applies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpToolScope {
    #[default]
    Global,
    Project,
    Session,
    Thread,
}

impl McpToolScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project => "project",
            Self::Session => "session",
            Self::Thread => "thread",
        }
    }
}

/// Permission granted by a binding for a server in a matching runtime context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpPermission {
    Connect,
    ListTools,
    CallTools,
    ReadResources,
}

impl McpPermission {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::ListTools => "list_tools",
            Self::CallTools => "call_tools",
            Self::ReadResources => "read_resources",
        }
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Explicit or compatibility binding between an MCP server and a runtime scope.
///
/// `server_id` is the stable runtime id used for permission checks. For unique
/// names it remains equal to the user-visible server name; discovery expands it
/// only when multiple sources contribute the same name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct McpBinding {
    pub server_id: String,
    pub scope: McpToolScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<McpPermission>,
    #[serde(skip_serializing_if = "is_false")]
    pub read_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
}

impl Default for McpBinding {
    fn default() -> Self {
        Self {
            server_id: String::new(),
            scope: McpToolScope::Global,
            project_path: None,
            session_id: None,
            thread_id: None,
            permissions: Vec::new(),
            read_only: false,
            source_scope: None,
        }
    }
}

impl McpBinding {
    pub fn full_permissions() -> Vec<McpPermission> {
        vec![
            McpPermission::Connect,
            McpPermission::ListTools,
            McpPermission::CallTools,
            McpPermission::ReadResources,
        ]
    }

    pub fn read_only_permissions() -> Vec<McpPermission> {
        vec![
            McpPermission::Connect,
            McpPermission::ListTools,
            McpPermission::ReadResources,
        ]
    }

    pub fn effective_permissions(&self) -> Vec<McpPermission> {
        if !self.permissions.is_empty() {
            return self.permissions.clone();
        }
        if self.read_only {
            Self::read_only_permissions()
        } else {
            Self::full_permissions()
        }
    }

    pub fn allows(&self, permission: McpPermission) -> bool {
        self.effective_permissions().contains(&permission)
    }

    pub fn identity_key(&self) -> String {
        let project = self
            .project_path
            .as_ref()
            .map(|path| normalize_path_for_key(path))
            .unwrap_or_default();
        format!(
            "{}|{}|{}|{}|{}",
            self.server_id,
            self.scope.as_str(),
            project,
            self.session_id.as_deref().unwrap_or_default(),
            self.thread_id.as_deref().unwrap_or_default()
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.server_id.trim().is_empty() {
            return Err("mcp binding serverId cannot be empty".to_string());
        }
        match self.scope {
            McpToolScope::Global => {}
            McpToolScope::Project => {
                if self.project_path.is_none() {
                    return Err("project-scoped mcp binding requires projectPath".to_string());
                }
            }
            McpToolScope::Session => {
                if self
                    .session_id
                    .as_deref()
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
                {
                    return Err("session-scoped mcp binding requires sessionId".to_string());
                }
            }
            McpToolScope::Thread => {
                if self
                    .session_id
                    .as_deref()
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
                {
                    return Err("thread-scoped mcp binding requires sessionId".to_string());
                }
                if self
                    .thread_id
                    .as_deref()
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
                {
                    return Err("thread-scoped mcp binding requires threadId".to_string());
                }
            }
        }
        Ok(())
    }
}

/// Runtime context used to decide which MCP bindings are visible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct McpBindingContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_path: Option<PathBuf>,
    pub session_id: String,
    pub thread_id: String,
    pub include_session_scope: bool,
    pub include_thread_scope: bool,
}

impl Default for McpBindingContext {
    fn default() -> Self {
        Self {
            project_path: None,
            session_id: String::new(),
            thread_id: "main".to_string(),
            include_session_scope: false,
            include_thread_scope: false,
        }
    }
}

impl McpBindingContext {
    pub fn startup(project_path: Option<PathBuf>) -> Self {
        Self {
            project_path,
            session_id: String::new(),
            thread_id: "main".to_string(),
            include_session_scope: false,
            include_thread_scope: false,
        }
    }

    pub fn main(project_path: Option<PathBuf>, session_id: impl Into<String>) -> Self {
        Self {
            project_path,
            session_id: session_id.into(),
            thread_id: "main".to_string(),
            include_session_scope: true,
            include_thread_scope: true,
        }
    }

    pub fn agent(
        project_path: Option<PathBuf>,
        session_id: impl Into<String>,
        thread_id: impl Into<String>,
    ) -> Self {
        Self {
            project_path,
            session_id: session_id.into(),
            thread_id: thread_id.into(),
            include_session_scope: false,
            include_thread_scope: true,
        }
    }

    pub fn matches_binding(&self, binding: &McpBinding) -> bool {
        match binding.scope {
            McpToolScope::Global => true,
            McpToolScope::Project => match (&binding.project_path, &self.project_path) {
                (Some(binding_path), Some(context_path)) => {
                    normalize_path_for_key(binding_path) == normalize_path_for_key(context_path)
                }
                _ => false,
            },
            McpToolScope::Session => {
                self.include_session_scope
                    && binding.session_id.as_deref() == Some(self.session_id.as_str())
            }
            McpToolScope::Thread => {
                self.include_thread_scope
                    && binding.session_id.as_deref() == Some(self.session_id.as_str())
                    && binding.thread_id.as_deref() == Some(self.thread_id.as_str())
            }
        }
    }
}

fn normalize_path_for_key(path: &Path) -> String {
    path.components()
        .as_path()
        .to_string_lossy()
        .trim_end_matches(std::path::MAIN_SEPARATOR)
        .to_string()
}
