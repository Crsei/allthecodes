use serde_json::Value;

use crate::v1;

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

    /// Send a chat message.
    Chat => "POST /api/chat" {
        params: v1::chat::ChatRequest,
        response: Value,
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

    PluginsList => "GET /api/plugins" {
        response: v1::plugins::PluginsListResponse,
    },
    PluginsMarketplace => "GET /api/plugins/marketplace" {
        response: v1::plugins::PluginsMarketplaceResponse,
    },
    PluginsInstall => "POST /api/plugins/install" {
        params: v1::plugins::PluginInstallRequest,
        response: v1::plugins::PluginInstallResponse,
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
        response: Value,
    },
    MemoryUpdate => "PATCH /api/memory/{id}" {
        params: Value,
        response: Value,
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
    ProvidersUpdate => "PATCH /api/providers/{id}" {
        params: v1::providers::ProviderUpdateRequest,
        response: Value,
    },
    ProvidersDelete => "DELETE /api/providers/{id}" {
        response: Value,
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
        response: Value,
    },
    LogsExport => "GET /api/logs/export" {
        response: Value,
    },
    DiagnosticsSnapshot => "GET /api/diagnostics/snapshot" {
        response: Value,
    },
    DiagnosticsTraces => "GET /api/diagnostics/traces" {
        response: Value,
    },

    TerminalProfiles => "GET /api/terminal/profiles" {
        response: EmptyResponse,
    },
    TerminalSessionsList => "GET /api/terminal/sessions" {
        response: EmptyResponse,
    },
    TerminalSessionsCreate => "POST /api/terminal/sessions" {
        response: EmptyResponse,
    },
    TerminalSessionDetail => "GET /api/terminal/sessions/{id}" {
        response: EmptyResponse,
    },
    TerminalSessionDelete => "DELETE /api/terminal/sessions/{id}" {
        response: EmptyResponse,
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
        response: Value,
    },
    GitDiff => "GET /api/git/diff" {
        response: Value,
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
        response: Value,
    },
    GroupChatRoomCreate => "POST /api/group-chat/rooms" {
        params: Value,
        response: Value,
    },
    GroupChatRoomDetail => "GET /api/group-chat/rooms/{id}" {
        response: Value,
    },
    GroupChatRoomDelete => "DELETE /api/group-chat/rooms/{id}" {
        response: Value,
    },
    GroupChatRoomClone => "POST /api/group-chat/rooms/{id}/clone" {
        response: Value,
    },
    GroupChatInvite => "GET /api/group-chat/rooms/{id}/invite" {
        response: Value,
    },
    GroupChatAgentAdd => "POST /api/group-chat/rooms/{id}/agents" {
        params: Value,
        response: Value,
    },
    GroupChatAgentUpdate => "PATCH /api/group-chat/rooms/{room_id}/agents/{agent_id}" {
        params: Value,
        response: Value,
    },
    GroupChatAgentDelete => "DELETE /api/group-chat/rooms/{room_id}/agents/{agent_id}" {
        response: Value,
    },
    GroupChatMessage => "POST /api/group-chat/rooms/{id}/messages" {
        params: Value,
        response: Value,
    },
    GroupChatCompression => "POST /api/group-chat/rooms/{id}/context-compression" {
        response: Value,
    },
    GroupChatStream => "GET /api/group-chat/rooms/{id}/stream" {
        response: Value,
    },

    BackendServices => "GET /api/backend-services" {
        response: Value,
    },
    BackendServicesSessionsSync => "POST /api/backend-services/sessions/sync" {
        response: Value,
    },
    BackendServicesContextCompressionRun => "POST /api/backend-services/context-compression/{id}/run" {
        response: Value,
    },
    BackendServicesAgentBridgeRetry => "POST /api/backend-services/agent-bridge/events/{id}/retry" {
        response: Value,
    },
    BackendServicesMigrationsRun => "POST /api/backend-services/migrations/run" {
        response: Value,
    },
    BackendServicesBackups => "POST /api/backend-services/backups" {
        params: Value,
        response: Value,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_metadata_contains_core_session_definitions_in_order() {
        let endpoints = [
            (ApiMethod::SessionList, "GET", "/api/sessions"),
            (ApiMethod::SessionCreate, "POST", "/api/sessions/new"),
            (ApiMethod::SessionDetail, "GET", "/api/sessions/{id}"),
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
