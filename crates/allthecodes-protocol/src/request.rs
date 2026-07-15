use serde_json::Value;

use crate::v1;

#[cfg(feature = "schema")]
pub fn schema_for<T>() -> schemars::schema::RootSchema
where
    T: schemars::JsonSchema,
{
    schemars::schema_for!(T)
}

pub const fn split_route(route: &'static str) -> (&'static str, &'static str) {
    let bytes = route.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b' ' {
            let (http_method, path_with_space) = route.split_at(index);
            let (_, path) = path_with_space.split_at(1);
            return (http_method, path);
        }

        index += 1;
    }

    (route, "")
}

pub fn serialization_key(value: Value) -> String {
    match value {
        Value::String(value) => value,
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

crate::api_definitions! {
    /// Check web backend health.
    Health => "GET /api/healthz" {
        response: v1::health::HealthResponse,
    },

    /// Fetch the effective KAIROS configuration and runtime lifecycle.
    KairosStatus => "GET /api/kairos" {
        response: v1::kairos::KairosResponse,
    },
    /// Persist a partial KAIROS profile and optionally reconcile the runtime.
    KairosConfigUpdate => "PUT /api/kairos/config" {
        params: v1::kairos::KairosConfigUpdateRequest,
        response: v1::kairos::KairosResponse,
        serialization: PerProcess,
    },
    /// Start KAIROS and wait for readiness.
    KairosStart => "POST /api/kairos/start" {
        params: v1::kairos::KairosControlParameters,
        response: v1::kairos::KairosResponse,
        serialization: PerProcess,
    },
    /// Stop KAIROS.
    KairosStop => "POST /api/kairos/stop" {
        params: v1::kairos::KairosControlParameters,
        response: v1::kairos::KairosResponse,
        serialization: PerProcess,
    },
    /// Restart KAIROS and wait for readiness.
    KairosRestart => "POST /api/kairos/restart" {
        params: v1::kairos::KairosControlParameters,
        response: v1::kairos::KairosResponse,
        serialization: PerProcess,
    },

    /// Send a chat message.
    Chat => "POST /api/chat" {
        params: v1::chat::ChatRequest,
        response: Value,
        stream: [v1::chat::ChatPermissionRequestEvent],
        serialization: PerKey("session_id"),
    },
    /// Abort an active chat request.
    Abort => "POST /api/abort" {
        params: v1::chat::AbortRequest,
        response: Value,
        serialization: PerKey("session_id"),
    },
    /// Respond to a pending chat tool permission request.
    ChatPermissionResponse => "POST /api/chat/permissions/{tool_use_id}/response" {
        params: v1::chat::ChatPermissionResponseRequest,
        response: Value,
        serialization: PerKey("session_id"),
    },
    /// Fetch current web UI state.
    State => "GET /api/state" {
        response: v1::chat::StateResponse,
    },
    /// Fetch the effective system prompt.
    SystemPrompt => "GET /api/system-prompt" {
        response: v1::chat::SystemPromptResponse,
    },
    /// Fetch coding agent availability status.
    CodingAgentsStatus => "GET /api/coding-agents/status" {
        response: Vec<v1::chat::CodingAgentStatus>,
    },

    /// Persist a Launch Pad snapshot under the global data root.
    LaunchpadSnapshotCreate => "POST /api/launchpad/snapshots" {
        params: v1::launchpad::LaunchpadSnapshotCreateRequest,
        response: v1::launchpad::LaunchpadSnapshotCreateResponse,
        serialization: PerProcess,
    },

    /// List active and archived sessions.
    SessionList => "GET /api/sessions" {
        response: v1::SessionListResponse,
    },
    /// Search saved session messages.
    SessionSearch => "GET /api/sessions/search" {
        params: v1::SessionSearchParams,
        response: v1::SessionSearchResponse,
    },
    /// Create a new session.
    SessionCreate => "POST /api/sessions/new" {
        params: v1::SessionCreateParams,
        response: v1::SessionCreateResponse,
        serialization: PerProcess,
    },
    /// Fetch one session by id.
    SessionDetail => "GET /api/sessions/{id}" {
        params: v1::SessionDetailParams,
        response: v1::SessionDetailResponse,
        errors: [NotFound],
        serialization: PerKey("id"),
    },
    /// Fetch the redacted runtime verification report for one session.
    SessionReport => "GET /api/sessions/{id}/report" {
        params: v1::SessionReportParams,
        response: v1::SessionReportResponse,
        serialization: PerKey("id"),
    },
    /// Resume an existing session.
    SessionResume => "POST /api/sessions/{id}/resume" {
        params: v1::SessionResumeParams,
        response: v1::SessionResumeResponse,
        errors: [NotFound, Conflict, EngineBusy],
        serialization: PerKey("id"),
    },
    /// Archive an existing session.
    SessionArchive => "POST /api/sessions/{id}/archive" {
        params: v1::SessionArchiveParams,
        response: v1::SessionArchiveResponse,
        errors: [NotFound, Conflict],
        serialization: PerKey("id"),
    },
    /// Set or clear a session chat mode override.
    SessionModePatch => "PATCH /api/sessions/{id}/mode" {
        params: v1::SessionModePatchParams,
        response: v1::SessionModePatchResponse,
        errors: [NotFound, Conflict],
        serialization: PerKey("id"),
    },

    /// Branch a session from a message.
    SessionMessageBranch => "POST /api/sessions/{id}/messages/{message_id}/branch" {
        params: v1::sessions::SessionMessageActionParams,
        response: Value,
        serialization: PerProcess,
    },
    /// Store feedback on a session message.
    SessionMessageFeedback => "POST /api/sessions/{id}/messages/{message_id}/feedback" {
        params: v1::sessions::SessionMessageActionParams,
        response: Value,
        serialization: PerProcess,
    },
    /// Delete a session message.
    SessionMessageDelete => "POST /api/sessions/{id}/messages/{message_id}/delete" {
        params: v1::sessions::SessionMessageActionParams,
        response: Value,
        serialization: PerProcess,
    },
    /// Prepare message regeneration.
    SessionMessageRegeneratePrepare => "POST /api/sessions/{id}/messages/{message_id}/regenerate/prepare" {
        params: v1::sessions::SessionMessageActionParams,
        response: Value,
        serialization: PerProcess,
    },
    /// Prepare message editing.
    SessionMessageEditPrepare => "POST /api/sessions/{id}/messages/{message_id}/edit/prepare" {
        params: v1::sessions::SessionMessageActionParams,
        response: Value,
        serialization: PerProcess,
    },
    /// Preview rollback to a message.
    SessionMessageRollbackPreview => "POST /api/sessions/{id}/messages/{message_id}/rollback/preview" {
        params: v1::sessions::SessionMessageActionParams,
        response: Value,
        serialization: PerProcess,
    },
    /// Roll back to a message.
    SessionMessageRollback => "POST /api/sessions/{id}/messages/{message_id}/rollback" {
        params: v1::sessions::SessionMessageActionParams,
        response: Value,
        serialization: PerProcess,
    },

    /// Discover backend capabilities.
    Capabilities => "GET /api/capabilities" {
        response: v1::capabilities::CapabilityDiscoveryResponse,
    },

    /// List chat modes.
    ChatModesList => "GET /api/chat-modes" {
        response: v1::chat_modes::ChatModesResponse,
    },
    /// List resources available to chat modes.
    ChatModesResources => "GET /api/chat-modes/resources" {
        response: v1::chat_modes::ChatModeResourcesResponse,
    },
    /// Create or update a chat mode.
    ChatModesUpsert => "PUT /api/chat-modes/{id}" {
        params: v1::chat_modes::ChatModeBundleUpsertRequest,
        response: Value,
    },
    /// Enable a chat mode and prepare its project resources.
    ChatModesEnable => "POST /api/chat-modes/{id}/enable" {
        response: Value,
    },
    /// Disable a chat mode and disable unshared mode plugins.
    ChatModesDisable => "POST /api/chat-modes/{id}/disable" {
        response: Value,
    },
    /// Delete a chat mode.
    ChatModesDelete => "DELETE /api/chat-modes/{id}" {
        response: Value,
    },

    /// List agents.
    AgentsList => "GET /api/agents" {
        response: v1::agents::AgentsListResponse,
    },
    /// Create an agent.
    AgentsCreate => "POST /api/agents" {
        params: v1::agents::AgentUpsertRequest,
        response: Value,
    },
    /// Fetch an agent.
    AgentsDetail => "GET /api/agents/{name}" {
        params: v1::agents::AgentDetailParams,
        response: Value,
    },
    /// Update an agent.
    AgentsUpdate => "PATCH /api/agents/{name}" {
        params: v1::agents::AgentUpsertRequest,
        response: Value,
    },
    /// Delete an agent.
    AgentsDelete => "DELETE /api/agents/{name}" {
        params: v1::agents::AgentDeleteParams,
        response: Value,
    },
    /// Restore an agent from overrides.
    AgentsRestore => "POST /api/agents/{name}/restore" {
        response: v1::agents::AgentRestoreResponse,
    },

    PeopleList => "GET /api/people" {
        response: v1::people::PeopleListResponse,
    },
    PeopleCreate => "POST /api/people" {
        params: v1::people::PersonCreateRequest,
        response: v1::people::PersonMutationResponse,
    },
    PeopleDetail => "GET /api/people/{id}" {
        response: v1::people::PersonMutationResponse,
    },
    PeopleUpdate => "PATCH /api/people/{id}" {
        params: v1::people::PersonUpdateRequest,
        response: v1::people::PersonMutationResponse,
    },
    PeopleDelete => "DELETE /api/people/{id}" {
        response: v1::people::PersonMutationResponse,
    },

    HooksList => "GET /api/hooks" {
        response: v1::hooks::HooksListResponse,
    },
    HooksCreate => "POST /api/hooks" {
        params: v1::hooks::HookEventRequest,
        response: v1::hooks::HookEventResponse,
    },
    HooksTest => "POST /api/hooks/test" {
        params: v1::hooks::HookEventRequest,
        response: v1::hooks::HookEventResponse,
    },
    HooksDetail => "GET /api/hooks/{event}" {
        response: v1::hooks::HookEventResponse,
    },
    HooksUpdate => "PATCH /api/hooks/{event}" {
        params: v1::hooks::HookEventUpdateRequest,
        response: v1::hooks::HookEventResponse,
    },
    HooksDelete => "DELETE /api/hooks/{event}" {
        response: v1::hooks::HookEventResponse,
    },

    PromptsList => "GET /api/prompts" {
        response: v1::prompts::PromptsListResponse,
    },
    PromptsCreate => "POST /api/prompts" {
        params: v1::prompts::PromptCreateRequest,
        response: v1::prompts::PromptMutationResponse,
    },
    PromptsDetail => "GET /api/prompts/{id}" {
        response: v1::prompts::PromptMutationResponse,
    },
    PromptsUpdate => "PATCH /api/prompts/{id}" {
        params: v1::prompts::PromptUpdateRequest,
        response: v1::prompts::PromptMutationResponse,
    },
    PromptsDelete => "DELETE /api/prompts/{id}" {
        response: v1::prompts::PromptMutationResponse,
    },

    McpServersList => "GET /api/mcp-servers" {
        response: Value,
    },
    McpServersHealth => "GET /api/mcp-servers/health" {
        response: Value,
    },
    McpServersProbe => "POST /api/mcp-servers/probe" {
        params: Value,
        response: Value,
    },
    McpServersCreate => "POST /api/mcp-servers" {
        params: Value,
        response: Value,
    },
    McpServersMarketplace => "GET /api/mcp-servers/marketplace" {
        response: Value,
    },
    McpServersDetail => "GET /api/mcp-servers/{name}" {
        response: Value,
    },
    McpServersUpdate => "PATCH /api/mcp-servers/{name}" {
        params: Value,
        response: Value,
    },
    McpServersDelete => "DELETE /api/mcp-servers/{name}" {
        response: Value,
    },
    /// Start MCP OAuth authorization flow with auto loopback callback.
    McpServersAuthStart => "POST /api/mcp-servers/{name}/oauth/start" {
        response: Value,
    },
    /// Complete MCP OAuth authorization manually with an authorization code.
    McpServersAuthComplete => "POST /api/mcp-servers/{name}/oauth/complete" {
        params: Value,
        response: Value,
    },
    /// Query redacted MCP OAuth credential status.
    McpServersAuthStatus => "GET /api/mcp-servers/{name}/oauth/status" {
        response: Value,
    },
    /// Clear stored MCP OAuth credentials.
    McpServersAuthClear => "DELETE /api/mcp-servers/{name}/oauth" {
        response: Value,
    },

    PluginsList => "GET /api/plugins" {
        response: v1::plugins::PluginsListResponse,
    },
    PluginsInstalled => "GET /api/plugins/installed" {
        response: v1::plugins::PluginsListResponse,
    },
    PluginsMarketplace => "GET /api/plugins/marketplace" {
        response: v1::plugins::PluginsMarketplaceResponse,
    },
    PluginsInstall => "POST /api/plugins/install" {
        params: v1::plugins::PluginInstallRequest,
        response: v1::plugins::PluginInstallResponse,
    },
    PluginsUpdate => "POST /api/plugins/update" {
        params: v1::plugins::PluginUpdateRequest,
        response: v1::plugins::PluginInstallResponse,
    },
    PluginsUninstallById => "POST /api/plugins/uninstall" {
        params: v1::plugins::PluginUninstallByIdRequest,
        response: v1::plugins::PluginUninstallResponse,
    },
    PluginsEnable => "POST /api/plugins/enable" {
        params: v1::plugins::PluginIdRequest,
        response: v1::plugins::PluginLifecycleResponse,
    },
    PluginsDisable => "POST /api/plugins/disable" {
        params: v1::plugins::PluginIdRequest,
        response: v1::plugins::PluginLifecycleResponse,
    },
    PluginsRestart => "POST /api/plugins/restart" {
        params: v1::plugins::PluginIdRequest,
        response: v1::plugins::PluginLifecycleResponse,
    },
    PluginsTestConnection => "POST /api/plugins/{id}/test-connection" {
        response: v1::plugins::PluginTestConnectionResponse,
    },
    PluginsUninstall => "POST /api/plugins/{id}/uninstall" {
        params: v1::plugins::PluginUninstallRequest,
        response: v1::plugins::PluginUninstallResponse,
    },

    ChannelsList => "GET /api/channels" {
        response: Value,
    },
    ChannelsCapabilities => "GET /api/channels/capabilities" {
        response: Value,
    },
    ChannelsConfig => "GET /api/channels/config" {
        response: v1::channels::ChannelsConfigResponse,
    },
    ChannelsConfigUpdate => "PATCH /api/channels/{provider}/config" {
        params: v1::channels::ChannelConfigPatch,
        response: v1::channels::ChannelConfigSnapshot,
    },
    ChannelsEnable => "POST /api/channels/{provider}/enable" {
        response: v1::channels::ChannelConfigSnapshot,
    },
    ChannelsDisable => "POST /api/channels/{provider}/disable" {
        response: v1::channels::ChannelConfigSnapshot,
    },
    ChannelsConnect => "POST /api/channels/{provider}/connect" {
        response: Value,
    },
    ChannelsTest => "POST /api/channels/{provider}/test" {
        params: Value,
        response: Value,
    },

    GatewayStatus => "GET /api/gateway/status" {
        response: v1::gateways::GatewayStatusResponse,
    },
    GatewaysList => "GET /api/gateways" {
        response: v1::gateways::GatewayListResponse,
    },
    GatewayStart => "POST /api/gateways/{id}/start" {
        params: v1::gateways::GatewayActionRequest,
        response: v1::gateways::GatewayActionResponse,
    },
    GatewayStop => "POST /api/gateways/{id}/stop" {
        params: v1::gateways::GatewayActionRequest,
        response: v1::gateways::GatewayActionResponse,
    },

    ComputerUseStatus => "GET /api/computer-use/status" {
        response: Value,
    },
    ComputerUsePermissionRequest => "POST /api/computer-use/permissions/{permission}/request" {
        response: Value,
    },
    ComputerUseTest => "POST /api/computer-use/test" {
        response: Value,
    },
    AppshotsStatus => "GET /api/appshots/status" {
        response: Value,
    },
    AppshotsCapture => "POST /api/appshots/capture" {
        response: Value,
    },
    ChromeRelayStatus => "GET /api/chrome-relay/status" {
        response: Value,
    },
    ChromeRelayLaunch => "POST /api/chrome-relay/launch" {
        response: Value,
    },
    ChromeRelayTokenRegenerate => "POST /api/chrome-relay/token/regenerate" {
        response: Value,
    },
    ActivityRecorderStatus => "GET /api/activity-recorder/status" {
        response: Value,
    },
    ActivityRecorderSessions => "GET /api/activity-recorder/sessions" {
        response: Value,
    },
    ActivityRecorderClear => "POST /api/activity-recorder/clear" {
        response: Value,
    },

    SettingsApply => "POST /api/settings" {
        params: Value,
        response: Value,
    },
    SettingsLayers => "GET /api/settings/layers" {
        response: Value,
    },
    CommandRun => "POST /api/command" {
        params: Value,
        response: Value,
    },
    MemoryConfigGet => "GET /api/memory/config" {
        response: Value,
    },
    MemoryConfigPatch => "PATCH /api/memory/config" {
        params: Value,
        response: Value,
    },
    MemoryList => "GET /api/memory" {
        params: v1::memory::MemoryListQuery,
        response: v1::memory::MemoryListResponse,
    },
    MemoryUpdate => "PATCH /api/memory/{id}" {
        params: v1::memory::MemoryUpdateRequest,
        response: v1::memory::MemoryUpdateResponse,
        errors: [NotFound],
        serialization: PerProcess,
    },
    MemoryDreamList => "GET /api/memory/dream" {
        params: v1::memory::MemoryDreamListQuery,
        response: v1::memory::MemoryDreamListResponse,
    },
    MemoryDreamDetail => "GET /api/memory/dream/{date}" {
        response: v1::memory::MemoryDreamDetailResponse,
        errors: [NotFound],
    },
    MemoryProposalList => "GET /api/memory/proposals" {
        params: v1::memory::MemoryProposalListQuery,
        response: v1::memory::MemoryProposalListResponse,
    },
    MemoryProposalDetail => "GET /api/memory/proposals/{id}" {
        response: v1::memory::MemoryProposalDetailResponse,
        errors: [NotFound],
    },
    MemoryProposalApprove => "POST /api/memory/proposals/{id}/approve" {
        response: v1::memory::MemoryProposalDecisionResponse,
        errors: [NotFound, Conflict],
        serialization: PerProcess,
    },
    MemoryProposalReject => "POST /api/memory/proposals/{id}/reject" {
        response: v1::memory::MemoryProposalDecisionResponse,
        errors: [NotFound, Conflict],
        serialization: PerProcess,
    },
    SpeechModels => "GET /api/speech/models" {
        response: Value,
    },
    SpeechModelDownload => "POST /api/speech/models/download" {
        params: Value,
        response: Value,
    },
    SpeechModelDelete => "DELETE /api/speech/models/{id}" {
        response: Value,
    },
    SearchCookiesExport => "POST /api/search/cookies/export" {
        response: Value,
    },
    SearchCookiesImport => "POST /api/search/cookies/import" {
        params: Value,
        response: Value,
    },
    SearchCookiesClear => "POST /api/search/cookies/clear" {
        response: Value,
    },
    DataExport => "POST /api/data/export" {
        response: Value,
    },
    DataImport => "POST /api/data/import" {
        params: Value,
        response: Value,
    },
    TokenSavings => "GET /api/token-savings" {
        response: Value,
    },
    DebugState => "GET /api/debug/state" {
        response: Value,
    },
    DebugSessionTrace => "GET /api/debug/sessions/{id}/trace" {
        response: Value,
    },
    DebugAction => "POST /api/debug/actions/{*action}" {
        params: Value,
        response: Value,
    },
    DiscoverySearch => "GET /api/discovery/search" {
        params: v1::discovery::DiscoverySearchQuery,
        response: v1::discovery::DiscoverySearchResponse,
        errors: [BadRequest, Internal],
    },
    ProtocolRoutes => "GET /api/-/routes" {
        response: Value,
    },

    WorkspacesList => "GET /api/workspaces" {
        response: v1::workspaces::WorkspacesResponse,
    },
    WorkspacePatch => "PATCH /api/workspaces/{workspace_key}" {
        params: v1::workspaces::WorkspacePatchRequest,
        response: v1::workspaces::WorkspaceMutationResponse,
    },
    WorkspaceOpen => "POST /api/workspaces/{workspace_key}/open" {
        params: v1::workspaces::WorkspaceOpenRequest,
        response: v1::workspaces::WorkspaceMutationResponse,
    },
    WorkspaceSessionsArchive => "POST /api/workspaces/{workspace_key}/sessions/archive" {
        params: v1::workspaces::WorkspaceArchiveRequest,
        response: v1::workspaces::WorkspaceArchiveResponse,
    },

    AuthStatus => "GET /api/auth/status" {
        response: Value,
    },
    AuthLogin => "POST /api/auth/login" {
        params: Value,
        response: Value,
    },
    AuthLogout => "POST /api/auth/logout" {
        response: Value,
    },
    AuthRefresh => "POST /api/auth/refresh" {
        response: Value,
    },
    AccountAuthLoginStart => "POST /api/account-auth/login/start" {
        params: v1::account_auth::AccountLoginStartRequest,
        response: v1::account_auth::AccountLoginStartResponse,
    },
    AccountAuthLoginComplete => "POST /api/account-auth/login/complete" {
        params: v1::account_auth::AccountLoginCompleteRequest,
        response: v1::account_auth::AccountAuthStatusResponse,
    },
    AccountAuthStatus => "GET /api/account-auth/status" {
        response: v1::account_auth::AccountAuthStatusResponse,
    },
    AccountAuthRefresh => "POST /api/account-auth/refresh" {
        response: v1::account_auth::AccountAuthStatusResponse,
    },
    AccountAuthLogout => "POST /api/account-auth/logout" {
        response: v1::account_auth::AccountAuthLogoutResponse,
    },
    AccountAuthBilling => "GET /api/account-auth/billing" {
        response: v1::account_auth::AccountBillingSnapshotResponse,
    },
    AccountAuthBillingLedger => "GET /api/account-auth/billing/ledger" {
        params: v1::account_auth::AccountBillingLedgerQuery,
        response: v1::account_auth::AccountBillingLedgerResponse,
    },
    AccountAuthBillingOrder => "GET /api/account-auth/billing/orders/{id}" {
        params: v1::account_auth::AccountBillingOrderParams,
        response: v1::account_auth::AccountBillingOrderResponse,
    },

    ProfilesList => "GET /api/profiles" {
        response: v1::profiles::ProfileListResponse,
    },
    ProfilesCreate => "POST /api/profiles" {
        params: v1::profiles::ProfileCreateRequest,
        response: Value,
    },
    ProfilesImport => "POST /api/profiles/import" {
        params: v1::profiles::ProfileImportRequest,
        response: Value,
    },
    ProfilesDetail => "GET /api/profiles/{id}" {
        response: Value,
    },
    ProfilesUpdate => "PATCH /api/profiles/{id}" {
        params: v1::profiles::ProfileUpdateRequest,
        response: Value,
    },
    ProfilesDelete => "DELETE /api/profiles/{id}" {
        response: Value,
    },
    ProfilesSwitch => "POST /api/profiles/{id}/switch" {
        response: Value,
    },
    ProfilesExport => "GET /api/profiles/{id}/export" {
        response: Value,
    },

    ProvidersList => "GET /api/providers" {
        response: v1::providers::ProviderListResponse,
    },
    ProvidersCreate => "POST /api/providers" {
        params: v1::providers::ProviderCreateRequest,
        response: Value,
    },
    ProvidersOpenaiCodexLocalStatus => "GET /api/providers/openai-codex/local-status" {
        response: v1::providers::CodexLocalStatusResponse,
    },
    ProvidersOpenaiCodexApplyLocal => "POST /api/providers/openai-codex/apply-local" {
        response: v1::providers::CodexApplyLocalResponse,
    },
    ProvidersUpdate => "PATCH /api/providers/{id}" {
        params: v1::providers::ProviderUpdateRequest,
        response: Value,
    },
    ProvidersDelete => "DELETE /api/providers/{id}" {
        response: Value,
    },
    ProvidersProbe => "POST /api/providers/{id}/probe" {
        params: v1::providers::ProviderProbeRequest,
        response: v1::providers::ProviderProbeResponse,
    },
    ProvidersRefreshModels => "POST /api/providers/{id}/models/refresh" {
        response: Value,
    },

    ModelsList => "GET /api/models" {
        response: v1::models::ModelRegistryResponse,
    },
    ModelsUpdate => "PATCH /api/models/{id}" {
        params: v1::models::ModelUpdateRequest,
        response: Value,
    },
    ModelsSetDefault => "POST /api/models/default" {
        params: v1::models::SetDefaultModelRequest,
        response: Value,
    },

    Credentials => "GET /api/credentials" {
        response: Value,
    },
    OAuthStart => "POST /api/oauth/{provider}/start" {
        params: Value,
        response: Value,
    },
    OAuthPoll => "POST /api/oauth/{provider}/poll" {
        response: Value,
    },
    Logs => "GET /api/logs" {
        params: v1::logs::LogsQuery,
        response: v1::logs::LogsResponse,
    },
    LogsExport => "GET /api/logs/export" {
        params: v1::logs::LogsExportQuery,
        response: Value,
    },
    DiagnosticsSnapshot => "GET /api/diagnostics/snapshot" {
        params: v1::logs::DiagnosticsQuery,
        response: v1::logs::DiagnosticsSnapshot,
    },
    DiagnosticsTraces => "GET /api/diagnostics/traces" {
        params: v1::logs::TracesQuery,
        response: v1::logs::TracesResponse,
    },

    TerminalHealth => "GET /api/terminal/healthz" {
        response: v1::terminal::TerminalHealthResponse,
    },
    TerminalProfiles => "GET /api/terminal/profiles" {
        response: v1::terminal::TerminalProfilesResponse,
    },
    TerminalSessionsList => "GET /api/terminal/sessions" {
        response: v1::terminal::TerminalSessionsResponse,
    },
    TerminalSessionsCreate => "POST /api/terminal/sessions" {
        params: v1::terminal::TerminalCreateRequest,
        response: v1::terminal::TerminalSessionSnapshot,
    },
    TerminalSessionDetail => "GET /api/terminal/sessions/{id}" {
        params: v1::terminal::TerminalSessionParams,
        response: v1::terminal::TerminalSessionSnapshot,
    },
    TerminalSessionOutput => "GET /api/terminal/sessions/{id}/output" {
        params: v1::terminal::TerminalOutputQuery,
        response: v1::terminal::TerminalOutputResponse,
    },
    TerminalSessionDelete => "DELETE /api/terminal/sessions/{id}" {
        params: v1::terminal::TerminalSessionParams,
        response: v1::terminal::TerminalSessionSnapshot,
    },
    TerminalSessionWs => "ANY /api/terminal/sessions/{id}/ws" {
        response: EmptyResponse,
    },
    TuiWs => "ANY /api/tui/ws" {
        response: EmptyResponse,
    },
    IpcWs => "ANY /api/ipc/ws" {
        response: EmptyResponse,
    },

    GitLog => "GET /api/git/log" {
        params: v1::git::GitLogParams,
        response: v1::git::GitLogResponse,
    },
    GitDiff => "GET /api/git/diff" {
        params: v1::git::GitDiffParams,
        response: v1::git::GitDiffResponse,
    },
    GitMetadata => "GET /api/git/metadata" {
        params: v1::git::GitMetadataParams,
        response: v1::git::GitMetadataResponse,
    },
    GitWorktrees => "GET /api/git/worktrees" {
        params: v1::git::GitWorktreesParams,
        response: v1::git::GitWorktreesResponse,
    },
    WorktreeSessionsList => "GET /api/worktree-sessions" {
        params: v1::worktree_sessions::WorktreeSessionsQuery,
        response: v1::worktree_sessions::WorktreeSessionsResponse,
    },
    WorktreeSessionsCurrent => "GET /api/worktree-sessions/current" {
        response: v1::worktree_sessions::CurrentWorktreeSessionResponse,
    },
    WorktreeSessionsBySession => "GET /api/worktree-sessions/{session_id}" {
        params: v1::worktree_sessions::WorktreeSessionBySessionParams,
        response: v1::worktree_sessions::WorktreeSessionsResponse,
    },
    Proxy => "GET /api/proxy" {
        response: Value,
    },
    Usage => "GET /api/usage" {
        response: Value,
    },

    FilesTree => "GET /api/files/tree" {
        params: v1::files::FileTreeQuery,
        response: v1::files::FileTreeResponse,
    },
    FilesStat => "GET /api/files/stat" {
        params: v1::files::FileStatQuery,
        response: v1::files::FileStat,
    },
    FilesRead => "GET /api/files/read" {
        params: v1::files::FileReadQuery,
        response: v1::files::FileReadResponse,
    },
    FilesPreview => "GET /api/files/preview" {
        params: v1::files::FilePreviewQuery,
        response: v1::files::FilePreviewResponse,
    },
    FilesMedia => "GET /api/files/media" {
        params: v1::files::FileMediaQuery,
        response: EmptyResponse,
    },
    FilesWrite => "PUT /api/files/write" {
        params: v1::files::FileWriteRequest,
        response: v1::files::FileMutationResponse,
        serialization: PerKey("path"),
    },
    FilesUpload => "POST /api/files/upload" {
        params: v1::files::FileUploadRequest,
        response: v1::files::FileUploadResponse,
        serialization: PerKey("path"),
    },
    FilesDownload => "GET /api/files/download" {
        params: v1::files::FileDownloadQuery,
        response: EmptyResponse,
    },
    FilesMkdir => "POST /api/files/mkdir" {
        params: v1::files::FileMkdirRequest,
        response: v1::files::FileMutationResponse,
        serialization: PerKey("path"),
    },
    FilesRename => "POST /api/files/rename" {
        params: v1::files::FileRenameRequest,
        response: v1::files::FileMutationResponse,
        serialization: PerKey("source"),
    },
    FilesCopy => "POST /api/files/copy" {
        params: v1::files::FileCopyRequest,
        response: v1::files::FileMutationResponse,
        serialization: PerKey("destination"),
    },
    FilesMove => "POST /api/files/move" {
        params: v1::files::FileMoveRequest,
        response: v1::files::FileMutationResponse,
        serialization: PerKey("source"),
    },
    FilesDelete => "DELETE /api/files" {
        params: v1::files::FileDeleteRequest,
        response: v1::files::FileMutationResponse,
        serialization: PerKey("path"),
    },

    SkillsList => "GET /api/skills" {
        params: v1::skills::SkillsListQuery,
        response: v1::skills::SkillsListResponse,
    },
    SkillProposalsList => "GET /api/skills/proposals" {
        params: v1::skills::SkillProposalListQuery,
        response: v1::skills::SkillProposalListResponse,
        errors: [BadRequest, Forbidden, PayloadTooLarge, ServiceUnavailable],
    },
    SkillProposalDetail => "GET /api/skills/proposals/{proposal_id}" {
        params: v1::skills::SkillProposalParams,
        response: v1::skills::SkillProposalDetailResponse,
        errors: [BadRequest, Forbidden, NotFound, Validation, PayloadTooLarge, ServiceUnavailable],
    },
    SkillProposalDiff => "GET /api/skills/proposals/{proposal_id}/diff" {
        params: v1::skills::SkillProposalParams,
        response: v1::skills::SkillProposalDiffResponse,
        errors: [BadRequest, Forbidden, NotFound, Validation, PayloadTooLarge, ServiceUnavailable],
    },
    SkillProposalApprove => "POST /api/skills/proposals/{proposal_id}/approve" {
        params: v1::skills::SkillProposalMutationParams,
        response: v1::skills::SkillProposalMutationResponse,
        errors: [BadRequest, Forbidden, NotFound, Conflict, Validation, PayloadTooLarge, ServiceUnavailable],
        serialization: PerKey("proposal_id"),
    },
    SkillProposalReject => "POST /api/skills/proposals/{proposal_id}/reject" {
        params: v1::skills::SkillProposalMutationParams,
        response: v1::skills::SkillProposalMutationResponse,
        errors: [BadRequest, Forbidden, NotFound, Conflict, Validation, PayloadTooLarge, ServiceUnavailable],
        serialization: PerKey("proposal_id"),
    },
    SkillsDetail => "GET /api/skills/{id}" {
        response: v1::skills::SkillDetailResponse,
    },
    SkillsPatch => "PATCH /api/skills/{id}" {
        params: v1::skills::SkillPatchRequest,
        response: v1::skills::SkillDetailResponse,
    },
    SkillsFiles => "GET /api/skills/{id}/files" {
        response: Value,
    },

    KanbanBoards => "GET /api/kanban/boards" {
        params: v1::kanban::KanbanQuery,
        response: v1::kanban::KanbanBoardsResponse,
    },
    KanbanBoardDetail => "GET /api/kanban/boards/{id}" {
        response: v1::kanban::KanbanBoardDetailResponse,
    },
    KanbanTaskCreate => "POST /api/kanban/tasks" {
        params: v1::kanban::KanbanTaskCreateRequest,
        response: v1::kanban::KanbanTaskMutationResponse,
        serialization: PerKey("board_id"),
    },
    KanbanTaskUpdate => "PATCH /api/kanban/tasks/{id}" {
        params: v1::kanban::KanbanTaskUpdateRequest,
        response: v1::kanban::KanbanTaskMutationResponse,
        serialization: PerProcess,
    },
    KanbanTaskComment => "POST /api/kanban/tasks/{id}/comments" {
        params: v1::kanban::KanbanCommentCreateRequest,
        response: v1::kanban::KanbanTaskMutationResponse,
        serialization: PerProcess,
    },

    WorkflowDefinitionsList => "GET /api/workflows" {
        params: v1::workflows::WorkflowDefinitionsQuery,
        response: v1::workflows::WorkflowDefinitionsResponse,
        errors: [BadRequest, Forbidden, PayloadTooLarge, ServiceUnavailable],
    },
    WorkflowDefinitionDetail => "GET /api/workflows/{workflow}" {
        params: v1::workflows::WorkflowDefinitionParams,
        response: v1::workflows::WorkflowDefinitionDetail,
        errors: [BadRequest, Forbidden, NotFound, PayloadTooLarge, ServiceUnavailable],
    },
    WorkflowRunsList => "GET /api/workflow-runs" {
        params: v1::workflows::WorkflowRunListQuery,
        response: v1::workflows::WorkflowRunPage,
        errors: [BadRequest, Forbidden, PayloadTooLarge, ServiceUnavailable],
    },
    WorkflowRunStart => "POST /api/workflows/{workflow}/runs" {
        params: v1::workflows::WorkflowRunStartParams,
        response: v1::workflows::WorkflowRunResponse,
        errors: [BadRequest, Forbidden, NotFound, Conflict, PayloadTooLarge, ServiceUnavailable],
        serialization: PerKey("workflow"),
    },
    WorkflowRunStatus => "GET /api/workflow-runs/{run_id}" {
        params: v1::workflows::WorkflowRunParams,
        response: v1::workflows::WorkflowRunResponse,
        errors: [BadRequest, Forbidden, NotFound, PayloadTooLarge, ServiceUnavailable],
    },
    WorkflowRunAdvance => "POST /api/workflow-runs/{run_id}/advance" {
        params: v1::workflows::WorkflowRunAdvanceParams,
        response: v1::workflows::WorkflowRunResponse,
        errors: [BadRequest, Forbidden, NotFound, Conflict, PayloadTooLarge, ServiceUnavailable],
        serialization: PerKey("run_id"),
    },
    WorkflowRunCancel => "POST /api/workflow-runs/{run_id}/cancel" {
        params: v1::workflows::WorkflowRunCancelParams,
        response: v1::workflows::WorkflowRunResponse,
        errors: [BadRequest, Forbidden, NotFound, Conflict, PayloadTooLarge, ServiceUnavailable],
        serialization: PerKey("run_id"),
    },

    JobsList => "GET /api/jobs" {
        response: Value,
    },
    JobsCreate => "POST /api/jobs" {
        params: Value,
        response: Value,
    },
    JobsUpdate => "PATCH /api/jobs/{id}" {
        params: Value,
        response: Value,
    },
    JobsDelete => "DELETE /api/jobs/{id}" {
        response: Value,
    },
    JobsPause => "POST /api/jobs/{id}/pause" {
        response: Value,
    },
    JobsResume => "POST /api/jobs/{id}/resume" {
        response: Value,
    },
    JobsRun => "POST /api/jobs/{id}/run" {
        response: Value,
    },
    CronHistory => "GET /api/cron/history" {
        response: Value,
    },

    GroupChatRoomsList => "GET /api/group-chat/rooms" {
        params: v1::group_chat::GroupChatProfileQuery,
        response: v1::group_chat::GroupChatRoomsResponse,
    },
    GroupChatRoomCreate => "POST /api/group-chat/rooms" {
        params: v1::group_chat::GroupChatRoomCreateRequest,
        response: v1::group_chat::GroupChatRoomMutationResponse,
        errors: [BadRequest, Conflict, PayloadTooLarge, ServiceUnavailable],
        serialization: PerProcess,
    },
    GroupChatRoomDetail => "GET /api/group-chat/rooms/{id}" {
        params: v1::group_chat::GroupChatRoomParams,
        response: v1::group_chat::GroupChatRoomDetailResponse,
        errors: [NotFound],
    },
    GroupChatRoomDelete => "DELETE /api/group-chat/rooms/{id}" {
        params: v1::group_chat::GroupChatRoomDeleteParams,
        response: v1::group_chat::GroupChatRoomMutationResponse,
        errors: [NotFound, Conflict],
        serialization: PerKey("id"),
    },
    GroupChatRoomClone => "POST /api/group-chat/rooms/{id}/clone" {
        params: v1::group_chat::GroupChatRoomCloneParams,
        response: v1::group_chat::GroupChatRoomMutationResponse,
        errors: [BadRequest, NotFound, Conflict, PayloadTooLarge],
        serialization: PerKey("id"),
    },
    GroupChatInvite => "GET /api/group-chat/rooms/{id}/invite" {
        params: v1::group_chat::GroupChatRoomParams,
        response: v1::group_chat::GroupChatInvite,
        errors: [NotFound],
    },
    GroupChatInviteMutation => "POST /api/group-chat/rooms/{id}/invite" {
        params: v1::group_chat::GroupChatInviteMutationParams,
        response: v1::group_chat::GroupChatInviteMutationResponse,
        errors: [BadRequest, NotFound, Conflict],
        serialization: PerKey("id"),
    },
    GroupChatAgentAdd => "POST /api/group-chat/rooms/{id}/agents" {
        params: v1::group_chat::GroupChatAgentCreateParams,
        response: v1::group_chat::GroupChatAgentMutationResponse,
        errors: [BadRequest, NotFound, Conflict],
        serialization: PerKey("room_id"),
    },
    GroupChatAgentUpdate => "PATCH /api/group-chat/rooms/{room_id}/agents/{agent_id}" {
        params: v1::group_chat::GroupChatAgentUpdateParams,
        response: v1::group_chat::GroupChatAgentMutationResponse,
        errors: [BadRequest, NotFound, Conflict],
        serialization: PerKey("room_id"),
    },
    GroupChatAgentDelete => "DELETE /api/group-chat/rooms/{room_id}/agents/{agent_id}" {
        params: v1::group_chat::GroupChatAgentDeleteParams,
        response: v1::group_chat::GroupChatAgentMutationResponse,
        errors: [NotFound, Conflict],
        serialization: PerKey("room_id"),
    },
    GroupChatMessage => "POST /api/group-chat/rooms/{id}/messages" {
        params: v1::group_chat::GroupChatMessageSendParams,
        response: v1::group_chat::GroupChatMessageResponse,
        errors: [BadRequest, NotFound, Conflict, PayloadTooLarge, ServiceUnavailable],
        serialization: PerKey("id"),
    },
    GroupChatCompression => "POST /api/group-chat/rooms/{id}/context-compression" {
        params: v1::group_chat::GroupChatCompressionUpdateParams,
        response: v1::group_chat::GroupChatCompressionResponse,
        errors: [BadRequest, NotFound, Conflict],
        serialization: PerKey("id"),
    },
    GroupChatStream => "GET /api/group-chat/rooms/{id}/stream" {
        params: v1::group_chat::GroupChatStreamQuery,
        response: v1::group_chat::GroupChatStreamEnvelope,
        stream: [v1::group_chat::GroupChatStreamEnvelope],
        errors: [NotFound],
    },

    BackendServices => "GET /api/backend-services" {
        params: v1::backend_services::BackendServicesQuery,
        response: v1::backend_services::BackendServicesResponse,
        errors: [BadRequest, Internal],
    },
    BackendServicesSessionsSync => "POST /api/backend-services/sessions/sync" {
        params: v1::backend_services::BackendServicesSessionSyncRequest,
        response: v1::backend_services::ServiceActionResponse,
        errors: [BadRequest, Internal],
        serialization: PerProcess,
    },
    BackendServicesContextCompressionRun => "POST /api/backend-services/context-compression/{id}/run" {
        params: v1::backend_services::BackendServicesCompressionRunParams,
        response: v1::backend_services::ServiceActionResponse,
        errors: [BadRequest, NotFound, Conflict, Validation, ServiceUnavailable],
        serialization: PerKey("id"),
    },
    BackendServicesAgentBridgeRetry => "POST /api/backend-services/agent-bridge/events/{id}/retry" {
        params: v1::backend_services::BackendServicesAgentRetryParams,
        response: v1::backend_services::ServiceActionResponse,
        errors: [BadRequest, NotFound, Conflict, Validation, ServiceUnavailable],
        serialization: PerKey("id"),
    },
    BackendServicesMigrationsRun => "POST /api/backend-services/migrations/run" {
        params: v1::backend_services::BackendServicesMigrationRequest,
        response: v1::backend_services::ServiceActionResponse,
        errors: [ServiceUnavailable],
        serialization: PerProcess,
    },
    BackendServicesBackups => "POST /api/backend-services/backups" {
        params: v1::backend_services::BackendServicesBackupRequest,
        response: v1::backend_services::ServiceActionResponse,
        errors: [ServiceUnavailable],
        serialization: PerProcess,
    },

    // ── Queue ────────────────────────────────────────────────────────────
    /// List all queued prompts.
    QueueList => "GET /api/queue" {
        response: v1::queue::QueueListResponse,
    },
    /// Add a prompt to the queue.
    QueueAdd => "POST /api/queue" {
        params: v1::queue::QueueAddRequest,
        response: v1::queue::QueueAddResponse,
        serialization: PerProcess,
    },
    /// Edit a queued prompt's text.
    QueueUpdate => "PATCH /api/queue/{id}" {
        params: v1::queue::QueueUpdateRequest,
        response: v1::queue::QueueUpdateResponse,
        serialization: PerProcess,
    },
    /// Remove a prompt from the queue.
    QueueRemove => "DELETE /api/queue/{id}" {
        response: v1::queue::QueueRemoveResponse,
        serialization: PerProcess,
    },
    /// Immediately send a queued prompt (move to front and submit).
    QueueSendNow => "POST /api/queue/{id}/send-now" {
        response: v1::queue::QueueSendNowResponse,
        serialization: PerProcess,
    },

    // ── Tasks ────────────────────────────────────────────────────────────
    /// List active tasks across sessions.
    TaskList => "GET /api/tasks" {
        response: v1::tasks::TaskListResponse,
    },
    /// Get detail for a single task.
    TaskDetail => "GET /api/tasks/{id}" {
        params: v1::tasks::TaskDetailParams,
        response: v1::tasks::TaskDetailResponse,
        errors: [NotFound],
    },

    // ── Agent runtime dashboard ──────────────────────────────────────────
    /// Fetch recent subagent runtime events and execution records.
    AgentRuntimeDashboard => "GET /api/agent-runtime/dashboard" {
        params: v1::agent_runtime::AgentRuntimeDashboardQuery,
        response: v1::agent_runtime::AgentRuntimeDashboardResponse,
    },

    // ── Mentions ─────────────────────────────────────────────────────────
    /// Autocomplete @mention chips (sessions, files, skills).
    MentionAutocomplete => "GET /api/mentions/autocomplete" {
        params: v1::mentions::MentionAutocompleteQuery,
        response: v1::mentions::MentionAutocompleteResponse,
    },

    // ── Sidebar pin / order ──────────────────────────────────────────────
    /// Pin a session to the sidebar.
    SidebarPin => "POST /api/sidebar/pin" {
        params: v1::sidebar::SidebarPinRequest,
        response: v1::sidebar::SidebarPinResponse,
        serialization: PerProcess,
    },
    /// Unpin a session from the sidebar.
    SidebarUnpin => "POST /api/sidebar/unpin" {
        params: v1::sidebar::SidebarUnpinRequest,
        response: v1::sidebar::SidebarUnpinResponse,
        serialization: PerProcess,
    },
    /// Reorder pinned and unpinned sessions in the sidebar.
    SidebarReorder => "POST /api/sidebar/reorder" {
        params: v1::sidebar::SidebarReorderRequest,
        response: v1::sidebar::SidebarReorderResponse,
        serialization: PerProcess,
    },

    // ── Cross-profile sessions ───────────────────────────────────────────
    /// List sessions across all profiles.
    SessionListAllProfiles => "GET /api/sessions/all-profiles" {
        response: v1::SessionListResponse,
    },

    // ── Messaging sections ───────────────────────────────────────────────
    /// List messaging sections.
    MessagingSectionsList => "GET /api/messaging/sections" {
        params: v1::messaging::MessagingSectionsQuery,
        response: v1::messaging::MessagingSectionsListResponse,
    },
    /// Create a messaging section.
    MessagingSectionCreate => "POST /api/messaging/sections" {
        params: v1::messaging::MessagingSectionCreateRequest,
        response: v1::messaging::MessagingSectionCreateResponse,
        serialization: PerProcess,
    },
    /// Update a messaging section.
    MessagingSectionUpdate => "PATCH /api/messaging/sections/{id}" {
        params: v1::messaging::MessagingSectionUpdateRequest,
        response: v1::messaging::MessagingSectionUpdateResponse,
        serialization: PerProcess,
    },
    /// Delete a messaging section.
    MessagingSectionDelete => "DELETE /api/messaging/sections/{id}" {
        response: v1::messaging::MessagingSectionDeleteResponse,
        serialization: PerProcess,
    },

    // ── Image generation ─────────────────────────────────────────────────
    /// Generate an image via an AI image provider.
    ImageGenerate => "POST /api/images/generate" {
        params: v1::image_generate::ImageGenerateRequest,
        response: v1::image_generate::ImageGenerateResponse,
        serialization: PerProcess,
    },

    // ── Voice (TTS / STT) ────────────────────────────────────────────────
    /// List available voice providers and voices.
    VoiceProviders => "GET /api/voice/providers" {
        response: v1::voice::VoiceProvidersResponse,
    },
    /// Convert text to speech via TTS.
    VoiceTts => "POST /api/voice/tts" {
        params: v1::voice::TtsRequest,
        response: v1::voice::TtsResponse,
        serialization: PerProcess,
    },
    /// Transcribe audio to text via STT.
    VoiceStt => "POST /api/voice/stt" {
        params: v1::voice::SttRequest,
        response: v1::voice::SttResponse,
        serialization: PerProcess,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_metadata_contains_core_session_definitions_in_order() {
        let endpoints = [
            (ApiMethod::SessionList, "GET", "/api/sessions"),
            (ApiMethod::SessionSearch, "GET", "/api/sessions/search"),
            (ApiMethod::SessionCreate, "POST", "/api/sessions/new"),
            (ApiMethod::SessionDetail, "GET", "/api/sessions/{id}"),
            (ApiMethod::SessionReport, "GET", "/api/sessions/{id}/report"),
            (
                ApiMethod::SessionResume,
                "POST",
                "/api/sessions/{id}/resume",
            ),
            (
                ApiMethod::SessionArchive,
                "POST",
                "/api/sessions/{id}/archive",
            ),
        ];

        for (operation, http_method, path) in endpoints {
            let endpoint = ALL_ENDPOINTS
                .iter()
                .find(|endpoint| endpoint.operation == operation)
                .expect("session endpoint should be declared");
            assert_eq!(endpoint.http_method, http_method);
            assert_eq!(endpoint.path, path);
            assert_eq!(operation.endpoint(), *endpoint);
        }
    }

    #[test]
    fn session_search_endpoint_metadata_is_declared() {
        let endpoint = ApiMethod::SessionSearch.endpoint();

        assert_eq!(endpoint.http_method, "GET");
        assert_eq!(endpoint.path, "/api/sessions/search");
    }

    #[test]
    fn endpoint_metadata_is_unique_by_operation_and_method_path() {
        use std::collections::HashSet;

        let mut operations = HashSet::new();
        let mut method_paths = HashSet::new();

        for endpoint in ALL_ENDPOINTS {
            assert!(
                operations.insert(endpoint.operation),
                "duplicate operation {:?}",
                endpoint.operation
            );
            assert!(
                method_paths.insert((endpoint.http_method, endpoint.path)),
                "duplicate route {} {}",
                endpoint.http_method,
                endpoint.path
            );
        }
    }

    #[test]
    fn multiple_methods_can_share_the_same_path() {
        let agents_path: Vec<&'static str> = ALL_ENDPOINTS
            .iter()
            .filter(|endpoint| endpoint.path == "/api/agents")
            .map(|endpoint| endpoint.http_method)
            .collect();

        assert_eq!(agents_path, vec!["GET", "POST"]);
    }

    #[test]
    fn mcp_oauth_endpoints_use_oauth_paths() {
        let endpoints = [
            (
                ApiMethod::McpServersAuthStart,
                "POST",
                "/api/mcp-servers/{name}/oauth/start",
            ),
            (
                ApiMethod::McpServersAuthComplete,
                "POST",
                "/api/mcp-servers/{name}/oauth/complete",
            ),
            (
                ApiMethod::McpServersAuthStatus,
                "GET",
                "/api/mcp-servers/{name}/oauth/status",
            ),
            (
                ApiMethod::McpServersAuthClear,
                "DELETE",
                "/api/mcp-servers/{name}/oauth",
            ),
        ];

        for (operation, http_method, path) in endpoints {
            let endpoint = operation.endpoint();
            assert_eq!(endpoint.http_method, http_method);
            assert_eq!(endpoint.path, path);
        }
    }

    #[test]
    fn websocket_and_any_routes_are_declared() {
        let any_routes: Vec<&'static str> = ALL_ENDPOINTS
            .iter()
            .filter(|endpoint| endpoint.http_method == "ANY")
            .map(|endpoint| endpoint.path)
            .collect();

        assert_eq!(
            any_routes,
            vec![
                "/api/terminal/sessions/{id}/ws",
                "/api/tui/ws",
                "/api/ipc/ws",
            ]
        );
        for path in any_routes {
            serde_json::to_string(&path).expect("route path should serialize");
        }
    }

    #[test]
    fn client_request_serde_roundtrip() {
        let request = ClientRequest::SessionDetail(v1::SessionDetailParams {
            id: "session-1".to_string(),
        });

        let encoded = serde_json::to_string(&request).unwrap();
        let decoded: ClientRequest = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, request);
        assert_eq!(decoded.endpoint().path, "/api/sessions/{id}");
    }

    #[test]
    fn client_response_serde_roundtrip() {
        let response = ClientResponse::SessionList(v1::SessionListResponse {
            sessions: vec![v1::SessionSummary {
                id: "session-1".to_string(),
                title: Some("Planning".to_string()),
                archived: false,
                chat_mode_override: None,
                effective_chat_mode: "normal".to_string(),
            }],
        });

        let encoded = serde_json::to_string(&response).unwrap();
        let decoded: ClientResponse = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, response);
    }

    #[test]
    fn serialization_scope_defaults_to_concurrent_and_reads_per_key_id() {
        assert_eq!(
            ClientRequest::SessionList(NoParams {}).serialization_scope(),
            SerializationScope::Concurrent
        );

        assert_eq!(
            ClientRequest::SessionResume(v1::SessionResumeParams {
                id: "session-2".to_string(),
            })
            .serialization_scope(),
            SerializationScope::PerKey {
                field: "id",
                key: "session-2".to_string(),
            }
        );
    }

    #[test]
    fn normal_endpoints_are_not_experimental() {
        assert_eq!(
            ClientRequest::SessionList(NoParams {}).experimental_reason(),
            None
        );
        assert_eq!(
            ClientRequest::SessionArchive(v1::SessionArchiveParams {
                id: "session-3".to_string(),
            })
            .experimental_reason(),
            None
        );
    }
}
