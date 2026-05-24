//! Per-subsystem event and command enums, plus a broadcast-based event bus.
//!
//! **Event enums** (Backend -> Frontend): `Serialize + Debug + Clone`, tagged by `"kind"`.
//! **Command enums** (Frontend -> Backend): `Deserialize + Debug`, tagged by `"kind"`.
//!
//! These types are consumed by:
//! - `protocol.rs` — wrapped inside `BackendMessage` / `FrontendMessage` variants
//! - `headless.rs` — event dispatch loop
//! - `subsystem_handlers.rs` — command handling
//! - LSP / MCP / Plugin / Skill modules — to emit events

use crate::lsp::{CompletionItemInfo, DocumentChange};
use crate::subsystem_types::*;
use serde::{Deserialize, Serialize};

// ===========================================================================
// Event enums (Backend → Frontend)
// ===========================================================================

/// Events emitted by the LSP subsystem.
#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LspEvent {
    /// An LSP server changed its lifecycle state.
    ServerStateChanged {
        language_id: String,
        state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Fresh diagnostics were published for a document.
    DiagnosticsPublished {
        uri: String,
        diagnostics: Vec<LspDiagnostic>,
    },
    /// Cached diagnostics snapshot, returned by `QueryDiagnostics`.
    DiagnosticsSnapshot {
        entries: Vec<LspDiagnosticSnapshotEntry>,
    },
    /// A live editor document was synced to its language server.
    DocumentSynced {
        uri: String,
        language_id: String,
        version: i32,
        change_kind: String,
    },
    /// Completion results for a live editor request.
    CompletionResults {
        request_id: String,
        uri: String,
        items: Vec<CompletionItemInfo>,
    },
    /// Async LSP command failure.
    CommandError {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        message: String,
    },
    /// Full list of LSP servers (response to `QueryStatus`).
    ServerList { servers: Vec<LspServerInfo> },
    /// Ask the frontend to show the "install suggested LSP" prompt.
    ///
    /// Mirrors upstream `LspRecommendationMenu`. The frontend replies via
    /// [`LspCommand::RecommendationResponse`].
    RecommendationRequest {
        #[serde(flatten)]
        payload: LspRecommendationPayload,
    },
    /// Current persisted settings for recommendation prompts (response to
    /// [`LspCommand::QuerySettings`]). Lets the frontend show "muted
    /// plugins" or "all disabled" state in an LSP settings view.
    SettingsSnapshot { settings: LspRecommendationSettings },
    /// User decision on an LSP recommendation — emitted for telemetry/audit
    /// after the frontend replies to a `RecommendationRequest`.
    RecommendationDecision {
        plugin_id: String,
        recommendation_id: String,
        /// One of "accepted", "dismissed", "remind_later".
        decision: String,
    },
}

/// Events emitted by the MCP subsystem.
#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpEvent {
    /// An MCP server changed its connection state.
    ServerStateChanged {
        server_name: String,
        state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Tools were (re-)discovered on an MCP server.
    ToolsDiscovered {
        server_name: String,
        tools: Vec<McpToolInfo>,
    },
    /// Resources were (re-)discovered on an MCP server.
    ResourcesDiscovered {
        server_name: String,
        resources: Vec<McpResourceInfo>,
    },
    /// A channel notification arrived from an MCP server.
    ChannelNotification {
        server_name: String,
        content: String,
        meta: serde_json::Value,
    },
    /// Full list of MCP servers (response to `QueryStatus`).
    ServerList { servers: Vec<McpServerStatusInfo> },
    /// Full list of editable config entries (response to `QueryConfig`).
    ///
    /// Distinct from `ServerList`: this is the settings-level view (source
    /// of truth for the editor) rather than the live runtime view.
    ConfigList { entries: Vec<McpServerConfigEntry> },
    /// A config entry was upserted or removed.
    ///
    /// `entry` is `None` when the server was deleted from the scope.
    ConfigChanged {
        server_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        entry: Option<Box<McpServerConfigEntry>>,
    },
    /// Config validation / persistence failure.
    ConfigError { server_name: String, error: String },
    /// OAuth authorization URL generated for an MCP server. Contains no token.
    AuthStarted {
        server_name: String,
        authorization_url: String,
        state: String,
        redirect_uri: String,
        token_store_path: String,
    },
    /// Redacted OAuth credential status for an MCP server.
    AuthStatus {
        server_name: String,
        configured: bool,
        authorized: bool,
        expired: bool,
        can_refresh: bool,
        token_store_path: String,
    },
}

/// Events emitted by the plugin subsystem.
#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginEvent {
    /// A plugin's status changed.
    StatusChanged {
        plugin_id: String,
        name: String,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Full list of plugins (response to `QueryStatus`).
    PluginList { plugins: Vec<PluginInfo> },
    /// Emitted when on-disk plugin state diverges from the in-memory
    /// registry and the session should call `/reload-plugins` to sync.
    ///
    /// `reason` is a short human-readable hint (`"installed_plugins.json changed"`,
    /// `"marketplace updated"`, etc.) that the UI can surface inline.
    RefreshNeeded { reason: String },
    /// Emitted after a reload cycle completes.
    ///
    /// `count` is the number of plugins in the registry post-reload.
    /// `had_error` is true when any plugin failed to load.
    Reloaded { count: usize, had_error: bool },

    // ── Phase 2 variants (Serial Integration Lane) ──
    /// A plugin was installed.
    Installed {
        plugin_id: String,
        name: String,
        version: String,
    },
    /// A plugin was updated to a new version.
    Updated {
        plugin_id: String,
        name: String,
        version: String,
    },
    /// A plugin was uninstalled.
    Uninstalled { plugin_id: String, name: String },
    /// Plugin validation failed.
    ValidationFailed {
        plugin_id: String,
        name: String,
        errors: Vec<String>,
    },
    /// Plugin configuration was changed externally.
    ConfigChanged { plugin_id: String, name: String },
}

/// Events emitted by the IDE-integration subsystem.
#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IdeEvent {
    /// Full list of detected IDEs, including the currently selected one
    /// (response to `QueryStatus`).
    IdeList { ides: Vec<IdeInfo> },
    /// The selected default IDE changed. `ide_id: None` means the
    /// selection was cleared.
    SelectionChanged {
        #[serde(skip_serializing_if = "Option::is_none")]
        ide_id: Option<String>,
    },
    /// Connection state of the bound IDE MCP bridge changed.
    ConnectionStateChanged {
        ide_id: String,
        state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

/// Events emitted by the skill subsystem.
#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillEvent {
    /// Skills were loaded / reloaded.
    SkillsLoaded { count: usize },
    /// Full list of skills (response to `QueryStatus`).
    SkillList { skills: Vec<SkillInfo> },
}

/// Events emitted by the agent-definition settings subsystem.
///
/// Distinct from `AgentEvent` (runtime agent lifecycle) — these drive the
/// `/agents` settings dialog.
#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentSettingsEvent {
    /// Full list of agent definitions (response to `QueryList`).
    List { entries: Vec<AgentDefinitionEntry> },
    /// An agent was upserted (entry present) or removed (entry `None`).
    Changed {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        entry: Option<Box<AgentDefinitionEntry>>,
    },
    /// Validation or persistence failure.
    Error { name: String, error: String },
    /// Categorized list of tools the frontend `ToolSelector` can show
    /// (response to `QueryTools`). Matches upstream `getToolBuckets()`.
    ToolList { tools: Vec<AgentToolInfo> },
    /// An external editor was launched and returned. The frontend should
    /// refresh the list afterwards — edits in the editor won't be reflected
    /// in the current snapshot until `QueryList` fires again.
    EditorOpened { file_path: String },
    /// Streamed progress marker for an AI-assisted generation run. The
    /// frontend shows "Generating…" between `GenerateStarted` and
    /// `Generated`/`Error`.
    GenerateStarted,
    /// An AI-generated agent definition (response to `Generate`).
    Generated {
        identifier: String,
        when_to_use: String,
        system_prompt: String,
    },
}

// ===========================================================================
// Command enums (Frontend → Backend)
// ===========================================================================

/// Commands the frontend can send to the LSP subsystem.
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LspCommand {
    StartServer {
        language_id: String,
    },
    StopServer {
        language_id: String,
    },
    RestartServer {
        language_id: String,
    },
    QueryStatus,
    /// Return the cached diagnostics for one URI or all tracked documents.
    QueryDiagnostics {
        #[serde(default)]
        uri: Option<String>,
    },
    /// Open or replace a live editor document on the language server.
    OpenDocument {
        uri: String,
        #[serde(default)]
        language_id: Option<String>,
        text: String,
    },
    /// Apply full-text or ranged changes to a live editor document.
    ChangeDocument {
        uri: String,
        #[serde(default)]
        version: Option<i32>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        changes: Vec<DocumentChange>,
    },
    /// Notify the language server that a live document was saved.
    SaveDocument {
        uri: String,
        #[serde(default)]
        text: Option<String>,
    },
    /// Close a live editor document.
    CloseDocument {
        uri: String,
    },
    /// Request completion results at a live editor position.
    Completion {
        request_id: String,
        uri: String,
        /// 1-based line number, matching the rest of the public LSP surface.
        line: u32,
        /// 1-based UTF-16 character offset, matching the rest of the public LSP surface.
        character: u32,
        #[serde(default)]
        trigger_character: Option<String>,
    },
    /// Return the currently-persisted recommendation settings as an
    /// [`LspEvent::SettingsSnapshot`].
    QuerySettings,
    /// User's reply to an [`LspEvent::RecommendationRequest`]. `decision`
    /// is one of `"yes"`, `"no"`, `"never"`, `"disable"`. `never` mutes
    /// future prompts for the named plugin; `disable` turns the whole
    /// prompt off. Both persist to the user-level `settings.json`.
    RecommendationResponse {
        request_id: String,
        plugin_name: String,
        decision: String,
    },
    /// Clear a previously-muted plugin from the "never recommend" list.
    /// Used by an LSP settings view to let the user undo a past
    /// `"never for X"` choice.
    UnmutePlugin {
        plugin_name: String,
    },
    /// Explicit toggle for the global "disable all recommendations"
    /// switch, so a settings view can flip it without having to
    /// synthesize a fake response.
    SetRecommendationsDisabled {
        disabled: bool,
    },
}

/// Commands the frontend can send to the MCP subsystem.
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpCommand {
    ConnectServer {
        server_name: String,
    },
    DisconnectServer {
        server_name: String,
    },
    ReconnectServer {
        server_name: String,
    },
    QueryStatus,
    /// Request the full list of editable config entries
    /// (drives the `/mcp` editor view).
    QueryConfig,
    /// Create or replace a config entry.
    ///
    /// The entry's `scope` determines which settings file is written.
    /// Non-editable scopes (`Plugin`/`Ide`) must be rejected by the
    /// handler with a `ConfigError` event.
    UpsertConfig {
        entry: Box<McpServerConfigEntry>,
    },
    /// Remove a config entry from the given scope.
    RemoveConfig {
        server_name: String,
        scope: ConfigScope,
    },
    /// Flip the `disabled` flag on an existing entry without removing it.
    ///
    /// When `scope` is `None` the backend picks the most specific editable
    /// match (project → user). Non-editable scopes (`Plugin`/`Ide`) are
    /// rejected with a `ConfigError` event.
    ToggleEnabled {
        server_name: String,
        #[serde(default)]
        scope: Option<ConfigScope>,
    },
    /// Start OAuth for a configured MCP server and return an authorization URL.
    StartAuth {
        server_name: String,
    },
    /// Complete OAuth using a code returned by the authorization server.
    CompleteAuth {
        server_name: String,
        code: String,
        #[serde(default)]
        state: Option<String>,
    },
    /// Clear stored OAuth token and any pending OAuth state.
    ClearAuth {
        server_name: String,
    },
    /// Return redacted OAuth credential status.
    QueryAuth {
        server_name: String,
    },
}

/// Commands the frontend can send to the plugin subsystem.
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginCommand {
    Enable {
        plugin_id: String,
    },
    Disable {
        plugin_id: String,
    },
    QueryStatus,
    /// Hot-refresh the plugin registry — drives `/reload-plugins`.
    ///
    /// Clears the in-memory registry and rebuilds from disk, then emits
    /// a `PluginEvent::Reloaded` with the new count.
    Reload,
    /// Uninstall a plugin (removes from `installed_plugins.json` and,
    /// when `purge_cache` is `true`, the local cache directory).
    Uninstall {
        plugin_id: String,
        #[serde(default)]
        purge_cache: bool,
    },
}

/// Commands the frontend can send to the IDE-integration subsystem.
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IdeCommand {
    /// Re-run host detection.
    Detect,
    /// Set the default IDE integration. Persists the selection and
    /// binds the corresponding MCP client.
    Select { ide_id: String },
    /// Clear the default IDE integration and disconnect the bridge.
    Clear,
    /// Reconnect the bound IDE MCP bridge.
    Reconnect,
    /// Query the current detection + selection state.
    QueryStatus,
}

/// Commands the frontend can send to the skill subsystem.
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillCommand {
    Reload,
    QueryStatus,
}

/// Commands the frontend can send to manage agent definitions.
///
/// Backs the `/agents` settings dialog; sibling to `McpCommand` for server
/// configs. `Builtin` and `Plugin` sources are read-only — upsert/delete calls
/// against those scopes must be rejected with `AgentSettingsEvent::Error`.
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentSettingsCommand {
    /// Return every known agent definition across all sources.
    QueryList,
    /// Create or replace an agent definition in its scope.
    Upsert { entry: Box<AgentDefinitionEntry> },
    /// Delete an agent definition from the given scope.
    Delete {
        name: String,
        source: AgentDefinitionSource,
    },
    /// Return the categorized tool list used by the `ToolSelector` UI.
    QueryTools,
    /// Launch `$EDITOR` against `file_path` — path must resolve inside an
    /// editable agents directory or the backend rejects it.
    OpenInEditor { file_path: String },
    /// Ask the model to generate a fresh agent definition from a
    /// user-supplied description. Mirrors upstream `generateAgent()`.
    Generate {
        /// Freeform description the user typed ("Help me write unit tests…").
        user_prompt: String,
        /// Names already in use — passed to the model so it picks a unique
        /// identifier. The frontend assembles this from the current list.
        #[serde(default)]
        existing_names: Vec<String>,
    },
}

// ===========================================================================
// Unified event wrapper
// ===========================================================================

/// Wrapper that tags every subsystem event with its origin subsystem.
#[derive(Debug, Clone)]
pub enum SubsystemEvent {
    Lsp(LspEvent),
    Mcp(McpEvent),
    Plugin(PluginEvent),
    Skill(SkillEvent),
    Ide(IdeEvent),
    AgentSettings(Box<AgentSettingsEvent>),

    // ── Phase 2 integration (Serial Integration Lane) ──
    /// A completion was provided/accepted by the user.
    CompletionProvided {
        request_id: String,
        accepted: bool,
        item_index: usize,
    },
    /// Request telemetry data flush (emitted by periodic timer or
    /// explicit `RefreshPluginTelemetry` frontend message).
    TelemetryFlush,
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Event serialization
    // -----------------------------------------------------------------------

    #[test]
    fn lsp_event_server_state_changed_serializes() {
        let event = LspEvent::ServerStateChanged {
            language_id: "rust".to_string(),
            state: "running".to_string(),
            error: None,
        };
        let value = serde_json::to_value(&event).expect("serialize LspEvent");
        assert_eq!(value["kind"], "server_state_changed");
        assert_eq!(value["language_id"], "rust");
        assert_eq!(value["state"], "running");
        assert!(value.get("error").is_none(), "None error should be omitted");
    }

    #[test]
    fn lsp_event_diagnostics_published_serializes() {
        let event = LspEvent::DiagnosticsPublished {
            uri: "file:///src/main.rs".to_string(),
            diagnostics: vec![LspDiagnostic {
                range: DiagnosticRange {
                    start_line: 1,
                    start_character: 0,
                    end_line: 1,
                    end_character: 10,
                },
                severity: "error".to_string(),
                message: "syntax error".to_string(),
                source: Some("rustc".to_string()),
                code: None,
            }],
        };
        let value = serde_json::to_value(&event).expect("serialize");
        assert_eq!(value["kind"], "diagnostics_published");
        assert_eq!(value["diagnostics"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn lsp_event_diagnostics_snapshot_serializes() {
        let event = LspEvent::DiagnosticsSnapshot {
            entries: vec![LspDiagnosticSnapshotEntry {
                uri: "file:///src/main.rs".to_string(),
                diagnostics: vec![],
            }],
        };
        let value = serde_json::to_value(&event).expect("serialize");
        assert_eq!(value["kind"], "diagnostics_snapshot");
        assert_eq!(value["entries"][0]["uri"], "file:///src/main.rs");
    }

    #[test]
    fn lsp_event_document_synced_serializes() {
        let event = LspEvent::DocumentSynced {
            uri: "file:///src/main.rs".to_string(),
            language_id: "rust".to_string(),
            version: 2,
            change_kind: "change".to_string(),
        };
        let value = serde_json::to_value(&event).expect("serialize");
        assert_eq!(value["kind"], "document_synced");
        assert_eq!(value["version"], 2);
        assert_eq!(value["change_kind"], "change");
    }

    #[test]
    fn lsp_event_completion_results_serializes() {
        let event = LspEvent::CompletionResults {
            request_id: "completion-1".to_string(),
            uri: "file:///src/main.rs".to_string(),
            items: vec![CompletionItemInfo {
                label: "println!".to_string(),
                kind: Some("Function".to_string()),
                detail: None,
                documentation: None,
                insert_text: Some("println!($0)".to_string()),
                sort_text: None,
                filter_text: None,
            }],
        };
        let value = serde_json::to_value(&event).expect("serialize");
        assert_eq!(value["kind"], "completion_results");
        assert_eq!(value["request_id"], "completion-1");
        assert_eq!(value["items"][0]["label"], "println!");
    }

    #[test]
    fn lsp_event_server_list_serializes() {
        let event = LspEvent::ServerList {
            servers: vec![LspServerInfo {
                language_id: "typescript".to_string(),
                state: "running".to_string(),
                extensions: vec![".ts".to_string()],
                open_files_count: 2,
                error: None,
            }],
        };
        let value = serde_json::to_value(&event).expect("serialize");
        assert_eq!(value["kind"], "server_list");
        assert_eq!(value["servers"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn lsp_event_recommendation_request_serializes_flat() {
        let event = LspEvent::RecommendationRequest {
            payload: LspRecommendationPayload {
                request_id: "req-1".into(),
                plugin_name: "rust-analyzer".into(),
                plugin_description: Some("Rust language server".into()),
                file_extension: ".rs".into(),
                language_id: Some("rust".into()),
            },
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["kind"], "recommendation_request");
        // `#[serde(flatten)]` means payload fields sit next to `kind`.
        assert_eq!(value["request_id"], "req-1");
        assert_eq!(value["plugin_name"], "rust-analyzer");
        assert_eq!(value["file_extension"], ".rs");
    }

    #[test]
    fn lsp_event_settings_snapshot_serializes() {
        let event = LspEvent::SettingsSnapshot {
            settings: LspRecommendationSettings {
                disabled: false,
                muted_plugins: vec!["pyright".into()],
            },
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["kind"], "settings_snapshot");
        assert_eq!(value["settings"]["disabled"], false);
        assert_eq!(value["settings"]["muted_plugins"][0], "pyright");
    }

    #[test]
    fn lsp_command_recommendation_response_deserializes() {
        let json = r#"{
            "kind":"recommendation_response",
            "request_id":"req-1",
            "plugin_name":"rust-analyzer",
            "decision":"never"
        }"#;
        let cmd: LspCommand = serde_json::from_str(json).expect("deserialize");
        match cmd {
            LspCommand::RecommendationResponse {
                request_id,
                plugin_name,
                decision,
            } => {
                assert_eq!(request_id, "req-1");
                assert_eq!(plugin_name, "rust-analyzer");
                assert_eq!(decision, "never");
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn lsp_command_unmute_plugin_deserializes() {
        let json = r#"{"kind":"unmute_plugin","plugin_name":"pyright"}"#;
        let cmd: LspCommand = serde_json::from_str(json).unwrap();
        match cmd {
            LspCommand::UnmutePlugin { plugin_name } => {
                assert_eq!(plugin_name, "pyright");
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn lsp_command_set_recommendations_disabled_deserializes() {
        let json = r#"{"kind":"set_recommendations_disabled","disabled":true}"#;
        let cmd: LspCommand = serde_json::from_str(json).unwrap();
        assert!(matches!(
            cmd,
            LspCommand::SetRecommendationsDisabled { disabled: true }
        ));
    }

    #[test]
    fn mcp_event_server_state_changed_serializes() {
        let event = McpEvent::ServerStateChanged {
            server_name: "context7".to_string(),
            state: "connected".to_string(),
            error: None,
        };
        let value = serde_json::to_value(&event).expect("serialize");
        assert_eq!(value["kind"], "server_state_changed");
        assert_eq!(value["server_name"], "context7");
        assert!(value.get("error").is_none());
    }

    #[test]
    fn mcp_event_tools_discovered_serializes() {
        let event = McpEvent::ToolsDiscovered {
            server_name: "test-server".to_string(),
            tools: vec![McpToolInfo {
                name: "search".to_string(),
                description: Some("Search docs".to_string()),
            }],
        };
        let value = serde_json::to_value(&event).expect("serialize");
        assert_eq!(value["kind"], "tools_discovered");
        assert_eq!(value["tools"][0]["name"], "search");
    }

    #[test]
    fn mcp_event_resources_discovered_serializes() {
        let event = McpEvent::ResourcesDiscovered {
            server_name: "res-server".to_string(),
            resources: vec![McpResourceInfo {
                uri: "file:///data.json".to_string(),
                name: Some("data".to_string()),
                mime_type: Some("application/json".to_string()),
            }],
        };
        let value = serde_json::to_value(&event).expect("serialize");
        assert_eq!(value["kind"], "resources_discovered");
        assert_eq!(value["resources"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn mcp_event_channel_notification_serializes() {
        let event = McpEvent::ChannelNotification {
            server_name: "chan-server".to_string(),
            content: "hello".to_string(),
            meta: serde_json::json!({"priority": "high"}),
        };
        let value = serde_json::to_value(&event).expect("serialize");
        assert_eq!(value["kind"], "channel_notification");
        assert_eq!(value["content"], "hello");
        assert_eq!(value["meta"]["priority"], "high");
    }

    #[test]
    fn mcp_event_server_list_serializes() {
        let event = McpEvent::ServerList {
            servers: vec![McpServerStatusInfo {
                name: "s1".to_string(),
                state: "connected".to_string(),
                transport: "stdio".to_string(),
                tools_count: 3,
                resources_count: 1,
                server_info: None,
                instructions: None,
                error: None,
            }],
        };
        let value = serde_json::to_value(&event).expect("serialize");
        assert_eq!(value["kind"], "server_list");
    }

    #[test]
    fn mcp_event_config_list_serializes() {
        let event = McpEvent::ConfigList {
            entries: vec![McpServerConfigEntry {
                name: "ctx7".into(),
                transport: "stdio".into(),
                command: Some("npx".into()),
                args: None,
                url: None,
                headers: None,
                oauth: None,
                env: None,
                browser_mcp: None,
                disabled: None,
                scope: ConfigScope::User,
            }],
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["kind"], "config_list");
        assert_eq!(value["entries"][0]["name"], "ctx7");
        assert_eq!(value["entries"][0]["scope"]["kind"], "user");
    }

    #[test]
    fn mcp_event_config_changed_with_none_entry_omits_field() {
        let event = McpEvent::ConfigChanged {
            server_name: "gone".into(),
            entry: None,
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["kind"], "config_changed");
        assert_eq!(value["server_name"], "gone");
        assert!(value.get("entry").is_none(), "None entry should be omitted");
    }

    #[test]
    fn mcp_event_config_error_serializes() {
        let event = McpEvent::ConfigError {
            server_name: "bad".into(),
            error: "invalid url".into(),
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["kind"], "config_error");
        assert_eq!(value["error"], "invalid url");
    }

    #[test]
    fn plugin_event_refresh_needed_serializes() {
        let event = PluginEvent::RefreshNeeded {
            reason: "installed_plugins.json changed".into(),
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["kind"], "refresh_needed");
        assert_eq!(value["reason"], "installed_plugins.json changed");
    }

    #[test]
    fn plugin_event_reloaded_serializes() {
        let event = PluginEvent::Reloaded {
            count: 4,
            had_error: false,
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["kind"], "reloaded");
        assert_eq!(value["count"], 4);
        assert_eq!(value["had_error"], false);
    }

    #[test]
    fn ide_event_ide_list_serializes() {
        let event = IdeEvent::IdeList {
            ides: vec![IdeInfo {
                id: "vscode".into(),
                name: "VS Code".into(),
                installed: true,
                running: false,
                selected: false,
                connection_state: None,
                error: None,
            }],
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["kind"], "ide_list");
        assert_eq!(value["ides"][0]["id"], "vscode");
    }

    #[test]
    fn ide_event_selection_changed_none_omits_field() {
        let event = IdeEvent::SelectionChanged { ide_id: None };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["kind"], "selection_changed");
        assert!(value.get("ide_id").is_none());
    }

    #[test]
    fn ide_event_connection_state_changed_serializes() {
        let event = IdeEvent::ConnectionStateChanged {
            ide_id: "cursor".into(),
            state: "connected".into(),
            error: None,
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["kind"], "connection_state_changed");
        assert_eq!(value["state"], "connected");
    }

    #[test]
    fn plugin_event_status_changed_serializes() {
        let event = PluginEvent::StatusChanged {
            plugin_id: "com.example.foo".to_string(),
            name: "Foo".to_string(),
            status: "active".to_string(),
            error: None,
        };
        let value = serde_json::to_value(&event).expect("serialize");
        assert_eq!(value["kind"], "status_changed");
        assert_eq!(value["plugin_id"], "com.example.foo");
        assert!(value.get("error").is_none());
    }

    #[test]
    fn plugin_event_status_changed_with_error_serializes() {
        let event = PluginEvent::StatusChanged {
            plugin_id: "com.example.broken".to_string(),
            name: "Broken".to_string(),
            status: "error".to_string(),
            error: Some("init failed".to_string()),
        };
        let value = serde_json::to_value(&event).expect("serialize");
        assert_eq!(value["error"], "init failed");
    }

    #[test]
    fn plugin_event_plugin_list_serializes() {
        let event = PluginEvent::PluginList {
            plugins: vec![PluginInfo {
                id: "test".to_string(),
                name: "Test".to_string(),
                version: "1.0.0".to_string(),
                status: "active".to_string(),
                contributed_tools: vec![],
                contributed_skills: vec![],
                contributed_mcp_servers: vec![],
                error: None,
            }],
        };
        let value = serde_json::to_value(&event).expect("serialize");
        assert_eq!(value["kind"], "plugin_list");
    }

    #[test]
    fn skill_event_skills_loaded_serializes() {
        let event = SkillEvent::SkillsLoaded { count: 5 };
        let value = serde_json::to_value(&event).expect("serialize");
        assert_eq!(value["kind"], "skills_loaded");
        assert_eq!(value["count"], 5);
    }

    #[test]
    fn skill_event_skill_list_serializes() {
        let event = SkillEvent::SkillList {
            skills: vec![SkillInfo {
                name: "simplify".to_string(),
                source: "builtin".to_string(),
                description: "Review code".to_string(),
                user_invocable: true,
                model_invocable: false,
            }],
        };
        let value = serde_json::to_value(&event).expect("serialize");
        assert_eq!(value["kind"], "skill_list");
        assert_eq!(value["skills"][0]["name"], "simplify");
    }

    // -----------------------------------------------------------------------
    // Command deserialization
    // -----------------------------------------------------------------------

    #[test]
    fn lsp_command_start_server_deserializes() {
        let json = r#"{"kind":"start_server","language_id":"rust"}"#;
        let cmd: LspCommand = serde_json::from_str(json).expect("deserialize");
        match cmd {
            LspCommand::StartServer { language_id } => assert_eq!(language_id, "rust"),
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn lsp_command_stop_server_deserializes() {
        let json = r#"{"kind":"stop_server","language_id":"typescript"}"#;
        let cmd: LspCommand = serde_json::from_str(json).expect("deserialize");
        match cmd {
            LspCommand::StopServer { language_id } => assert_eq!(language_id, "typescript"),
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn lsp_command_restart_server_deserializes() {
        let json = r#"{"kind":"restart_server","language_id":"python"}"#;
        let cmd: LspCommand = serde_json::from_str(json).expect("deserialize");
        match cmd {
            LspCommand::RestartServer { language_id } => assert_eq!(language_id, "python"),
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn lsp_command_query_status_deserializes() {
        let json = r#"{"kind":"query_status"}"#;
        let cmd: LspCommand = serde_json::from_str(json).expect("deserialize");
        assert!(matches!(cmd, LspCommand::QueryStatus));
    }

    #[test]
    fn lsp_command_query_diagnostics_deserializes() {
        let json = r#"{"kind":"query_diagnostics","uri":"file:///src/main.rs"}"#;
        let cmd: LspCommand = serde_json::from_str(json).expect("deserialize");
        match cmd {
            LspCommand::QueryDiagnostics { uri } => {
                assert_eq!(uri.as_deref(), Some("file:///src/main.rs"));
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn lsp_command_change_document_deserializes() {
        let json = r#"{
            "kind":"change_document",
            "uri":"file:///src/main.rs",
            "version":3,
            "changes":[{
                "range":{
                    "start_line":1,
                    "start_character":4,
                    "end_line":1,
                    "end_character":4
                },
                "text":"pub "
            }]
        }"#;
        let cmd: LspCommand = serde_json::from_str(json).expect("deserialize");
        match cmd {
            LspCommand::ChangeDocument {
                uri,
                version,
                changes,
                ..
            } => {
                assert_eq!(uri, "file:///src/main.rs");
                assert_eq!(version, Some(3));
                assert_eq!(changes.len(), 1);
                assert_eq!(changes[0].range.start_character, 4);
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn lsp_command_completion_deserializes() {
        let json = r#"{
            "kind":"completion",
            "request_id":"c1",
            "uri":"file:///src/main.rs",
            "line":10,
            "character":5,
            "trigger_character":"."
        }"#;
        let cmd: LspCommand = serde_json::from_str(json).expect("deserialize");
        match cmd {
            LspCommand::Completion {
                request_id,
                line,
                character,
                trigger_character,
                ..
            } => {
                assert_eq!(request_id, "c1");
                assert_eq!(line, 10);
                assert_eq!(character, 5);
                assert_eq!(trigger_character.as_deref(), Some("."));
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn mcp_command_connect_server_deserializes() {
        let json = r#"{"kind":"connect_server","server_name":"ctx7"}"#;
        let cmd: McpCommand = serde_json::from_str(json).expect("deserialize");
        match cmd {
            McpCommand::ConnectServer { server_name } => assert_eq!(server_name, "ctx7"),
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn mcp_command_disconnect_server_deserializes() {
        let json = r#"{"kind":"disconnect_server","server_name":"broken"}"#;
        let cmd: McpCommand = serde_json::from_str(json).expect("deserialize");
        assert!(matches!(cmd, McpCommand::DisconnectServer { .. }));
    }

    #[test]
    fn mcp_command_reconnect_server_deserializes() {
        let json = r#"{"kind":"reconnect_server","server_name":"s1"}"#;
        let cmd: McpCommand = serde_json::from_str(json).expect("deserialize");
        assert!(matches!(cmd, McpCommand::ReconnectServer { .. }));
    }

    #[test]
    fn mcp_command_query_status_deserializes() {
        let json = r#"{"kind":"query_status"}"#;
        let cmd: McpCommand = serde_json::from_str(json).expect("deserialize");
        assert!(matches!(cmd, McpCommand::QueryStatus));
    }

    #[test]
    fn mcp_command_complete_auth_deserializes() {
        let json =
            r#"{"kind":"complete_auth","server_name":"remote","code":"abc","state":"state-1"}"#;
        let cmd: McpCommand = serde_json::from_str(json).expect("deserialize");
        match cmd {
            McpCommand::CompleteAuth {
                server_name,
                code,
                state,
            } => {
                assert_eq!(server_name, "remote");
                assert_eq!(code, "abc");
                assert_eq!(state.as_deref(), Some("state-1"));
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn plugin_command_enable_deserializes() {
        let json = r#"{"kind":"enable","plugin_id":"com.example.foo"}"#;
        let cmd: PluginCommand = serde_json::from_str(json).expect("deserialize");
        match cmd {
            PluginCommand::Enable { plugin_id } => assert_eq!(plugin_id, "com.example.foo"),
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn plugin_command_disable_deserializes() {
        let json = r#"{"kind":"disable","plugin_id":"com.example.bar"}"#;
        let cmd: PluginCommand = serde_json::from_str(json).expect("deserialize");
        assert!(matches!(cmd, PluginCommand::Disable { .. }));
    }

    #[test]
    fn plugin_command_query_status_deserializes() {
        let json = r#"{"kind":"query_status"}"#;
        let cmd: PluginCommand = serde_json::from_str(json).expect("deserialize");
        assert!(matches!(cmd, PluginCommand::QueryStatus));
    }

    #[test]
    fn skill_command_reload_deserializes() {
        let json = r#"{"kind":"reload"}"#;
        let cmd: SkillCommand = serde_json::from_str(json).expect("deserialize");
        assert!(matches!(cmd, SkillCommand::Reload));
    }

    #[test]
    fn skill_command_query_status_deserializes() {
        let json = r#"{"kind":"query_status"}"#;
        let cmd: SkillCommand = serde_json::from_str(json).expect("deserialize");
        assert!(matches!(cmd, SkillCommand::QueryStatus));
    }

    #[test]
    fn subsystem_event_wraps_all_variants() {
        // Ensure we can construct every SubsystemEvent variant (compile-time check).
        let _lsp = SubsystemEvent::Lsp(LspEvent::ServerList { servers: vec![] });
        let _mcp = SubsystemEvent::Mcp(McpEvent::ServerList { servers: vec![] });
        let _plugin = SubsystemEvent::Plugin(PluginEvent::PluginList { plugins: vec![] });
        let _skill = SubsystemEvent::Skill(SkillEvent::SkillList { skills: vec![] });
        let _agents =
            SubsystemEvent::AgentSettings(Box::new(AgentSettingsEvent::List { entries: vec![] }));
    }
}
