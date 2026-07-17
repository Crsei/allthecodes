# allthecodes API Routes

Generated from `allthecodes-protocol` metadata.

Protocol digest: `5d5f8f896f51ad62bcbc5fc027656065f6f40a0711e4a98a6333a34fbd804893`.

| Operation | Method | Path | Transport | Params | Response | Stream events | Serialization | Errors | Experimental |
|---|---|---|---|---|---|---|---|---|---|
| `Health` | `GET` | `/api/healthz` | - | - | `HealthResponse` | - | `concurrent` | - | - |
| `KairosStatus` | `GET` | `/api/kairos` | - | - | `KairosResponse` | - | `concurrent` | - | - |
| `KairosConfigUpdate` | `PUT` | `/api/kairos/config` | - | `KairosConfigUpdateRequest` | `KairosResponse` | - | `per-process` | - | - |
| `KairosStart` | `POST` | `/api/kairos/start` | - | `KairosControlParameters` | `KairosResponse` | - | `per-process` | - | - |
| `KairosStop` | `POST` | `/api/kairos/stop` | - | `KairosControlParameters` | `KairosResponse` | - | `per-process` | - | - |
| `KairosRestart` | `POST` | `/api/kairos/restart` | - | `KairosControlParameters` | `KairosResponse` | - | `per-process` | - | - |
| `Chat` | `POST` | `/api/chat` | - | `ChatRequest` | `JsonValue` | `ChatPermissionRequestEvent` | `per-key:session_id` | - | - |
| `Abort` | `POST` | `/api/abort` | - | `AbortRequest` | `JsonValue` | - | `per-key:session_id` | - | - |
| `ChatPermissionResponse` | `POST` | `/api/chat/permissions/{tool_use_id}/response` | - | `ChatPermissionResponseRequest` | `JsonValue` | - | `per-key:session_id` | - | - |
| `State` | `GET` | `/api/state` | - | - | `StateResponse` | - | `concurrent` | - | - |
| `SystemPrompt` | `GET` | `/api/system-prompt` | - | - | `SystemPromptResponse` | - | `concurrent` | - | - |
| `CodingAgentsStatus` | `GET` | `/api/coding-agents/status` | - | - | `CodingAgentStatus[]` | - | `concurrent` | - | - |
| `LaunchpadSnapshotCreate` | `POST` | `/api/launchpad/snapshots` | - | `LaunchpadSnapshotCreateRequest` | `LaunchpadSnapshotCreateResponse` | - | `per-process` | - | - |
| `SessionList` | `GET` | `/api/sessions` | - | - | `SessionListResponse` | - | `concurrent` | - | - |
| `SessionSearch` | `GET` | `/api/sessions/search` | - | `SessionSearchParams` | `SessionSearchResponse` | - | `concurrent` | - | - |
| `SessionCreate` | `POST` | `/api/sessions/new` | - | `SessionCreateParams` | `SessionCreateResponse` | - | `per-process` | - | - |
| `SessionDetail` | `GET` | `/api/sessions/{id}` | - | `SessionDetailParams` | `SessionDetailResponse` | - | `per-key:id` | `NotFound` | - |
| `SessionReport` | `GET` | `/api/sessions/{id}/report` | - | `SessionReportParams` | `SessionReportResponse` | - | `per-key:id` | - | - |
| `SessionResume` | `POST` | `/api/sessions/{id}/resume` | - | `SessionResumeParams` | `SessionResumeResponse` | - | `per-key:id` | `NotFound`, `Conflict`, `EngineBusy` | - |
| `SessionArchive` | `POST` | `/api/sessions/{id}/archive` | - | `SessionArchiveParams` | `SessionArchiveResponse` | - | `per-key:id` | `NotFound`, `Conflict` | - |
| `SessionModePatch` | `PATCH` | `/api/sessions/{id}/mode` | - | `SessionModePatchParams` | `SessionModePatchResponse` | - | `per-key:id` | `NotFound`, `Conflict` | - |
| `SessionMessageBranch` | `POST` | `/api/sessions/{id}/messages/{message_id}/branch` | - | `SessionMessageActionParams` | `JsonValue` | - | `per-process` | - | - |
| `SessionMessageFeedback` | `POST` | `/api/sessions/{id}/messages/{message_id}/feedback` | - | `SessionMessageActionParams` | `JsonValue` | - | `per-process` | - | - |
| `SessionMessageDelete` | `POST` | `/api/sessions/{id}/messages/{message_id}/delete` | - | `SessionMessageActionParams` | `JsonValue` | - | `per-process` | - | - |
| `SessionMessageRegeneratePrepare` | `POST` | `/api/sessions/{id}/messages/{message_id}/regenerate/prepare` | - | `SessionMessageActionParams` | `JsonValue` | - | `per-process` | - | - |
| `SessionMessageEditPrepare` | `POST` | `/api/sessions/{id}/messages/{message_id}/edit/prepare` | - | `SessionMessageActionParams` | `JsonValue` | - | `per-process` | - | - |
| `SessionMessageRollbackPreview` | `POST` | `/api/sessions/{id}/messages/{message_id}/rollback/preview` | - | `SessionMessageActionParams` | `JsonValue` | - | `per-process` | - | - |
| `SessionMessageRollback` | `POST` | `/api/sessions/{id}/messages/{message_id}/rollback` | - | `SessionMessageActionParams` | `JsonValue` | - | `per-process` | - | - |
| `Capabilities` | `GET` | `/api/capabilities` | - | - | `CapabilityDiscoveryResponse` | - | `concurrent` | - | - |
| `ChatModesList` | `GET` | `/api/chat-modes` | - | - | `ChatModesResponse` | - | `concurrent` | - | - |
| `ChatModesResources` | `GET` | `/api/chat-modes/resources` | - | - | `ChatModeResourcesResponse` | - | `concurrent` | - | - |
| `ChatModesUpsert` | `PUT` | `/api/chat-modes/{id}` | - | `ChatModeBundleUpsertRequest` | `JsonValue` | - | `concurrent` | - | - |
| `ChatModesEnable` | `POST` | `/api/chat-modes/{id}/enable` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ChatModesDisable` | `POST` | `/api/chat-modes/{id}/disable` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ChatModesDelete` | `DELETE` | `/api/chat-modes/{id}` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `AgentsList` | `GET` | `/api/agents` | - | - | `AgentsListResponse` | - | `concurrent` | - | - |
| `AgentsCreate` | `POST` | `/api/agents` | - | `AgentUpsertRequest` | `JsonValue` | - | `concurrent` | - | - |
| `AgentsDetail` | `GET` | `/api/agents/{name}` | - | `AgentDetailParams` | `JsonValue` | - | `concurrent` | - | - |
| `AgentsUpdate` | `PATCH` | `/api/agents/{name}` | - | `AgentUpsertRequest` | `JsonValue` | - | `concurrent` | - | - |
| `AgentsDelete` | `DELETE` | `/api/agents/{name}` | - | `AgentDeleteParams` | `JsonValue` | - | `concurrent` | - | - |
| `AgentsRestore` | `POST` | `/api/agents/{name}/restore` | - | - | `AgentRestoreResponse` | - | `concurrent` | - | - |
| `PeopleList` | `GET` | `/api/people` | - | - | `PeopleListResponse` | - | `concurrent` | - | - |
| `PeopleCreate` | `POST` | `/api/people` | - | `PersonCreateRequest` | `PersonMutationResponse` | - | `concurrent` | - | - |
| `PeopleDetail` | `GET` | `/api/people/{id}` | - | - | `PersonMutationResponse` | - | `concurrent` | - | - |
| `PeopleUpdate` | `PATCH` | `/api/people/{id}` | - | `PersonUpdateRequest` | `PersonMutationResponse` | - | `concurrent` | - | - |
| `PeopleDelete` | `DELETE` | `/api/people/{id}` | - | - | `PersonMutationResponse` | - | `concurrent` | - | - |
| `HooksList` | `GET` | `/api/hooks` | - | - | `HooksListResponse` | - | `concurrent` | - | - |
| `HooksCreate` | `POST` | `/api/hooks` | - | `HookEventRequest` | `HookEventResponse` | - | `concurrent` | - | - |
| `HooksTest` | `POST` | `/api/hooks/test` | - | `HookEventRequest` | `HookEventResponse` | - | `concurrent` | - | - |
| `HooksDetail` | `GET` | `/api/hooks/{event}` | - | - | `HookEventResponse` | - | `concurrent` | - | - |
| `HooksUpdate` | `PATCH` | `/api/hooks/{event}` | - | `HookEventUpdateRequest` | `HookEventResponse` | - | `concurrent` | - | - |
| `HooksDelete` | `DELETE` | `/api/hooks/{event}` | - | - | `HookEventResponse` | - | `concurrent` | - | - |
| `PromptsList` | `GET` | `/api/prompts` | - | - | `PromptsListResponse` | - | `concurrent` | - | - |
| `PromptsCreate` | `POST` | `/api/prompts` | - | `PromptCreateRequest` | `PromptMutationResponse` | - | `concurrent` | - | - |
| `PromptsDetail` | `GET` | `/api/prompts/{id}` | - | - | `PromptMutationResponse` | - | `concurrent` | - | - |
| `PromptsUpdate` | `PATCH` | `/api/prompts/{id}` | - | `PromptUpdateRequest` | `PromptMutationResponse` | - | `concurrent` | - | - |
| `PromptsDelete` | `DELETE` | `/api/prompts/{id}` | - | - | `PromptMutationResponse` | - | `concurrent` | - | - |
| `McpServersList` | `GET` | `/api/mcp-servers` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `McpServersHealth` | `GET` | `/api/mcp-servers/health` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `McpServersProbe` | `POST` | `/api/mcp-servers/probe` | - | `JsonValue` | `JsonValue` | - | `concurrent` | - | - |
| `McpServersCreate` | `POST` | `/api/mcp-servers` | - | `JsonValue` | `JsonValue` | - | `concurrent` | - | - |
| `McpServersMarketplace` | `GET` | `/api/mcp-servers/marketplace` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `McpServersDetail` | `GET` | `/api/mcp-servers/{name}` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `McpServersUpdate` | `PATCH` | `/api/mcp-servers/{name}` | - | `JsonValue` | `JsonValue` | - | `concurrent` | - | - |
| `McpServersDelete` | `DELETE` | `/api/mcp-servers/{name}` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `McpServersAuthStart` | `POST` | `/api/mcp-servers/{name}/oauth/start` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `McpServersAuthComplete` | `POST` | `/api/mcp-servers/{name}/oauth/complete` | - | `JsonValue` | `JsonValue` | - | `concurrent` | - | - |
| `McpServersAuthStatus` | `GET` | `/api/mcp-servers/{name}/oauth/status` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `McpServersAuthClear` | `DELETE` | `/api/mcp-servers/{name}/oauth` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `PluginsList` | `GET` | `/api/plugins` | - | - | `PluginsListResponse` | - | `concurrent` | - | - |
| `PluginsInstalled` | `GET` | `/api/plugins/installed` | - | - | `PluginsListResponse` | - | `concurrent` | - | - |
| `PluginsMarketplace` | `GET` | `/api/plugins/marketplace` | - | - | `PluginsMarketplaceResponse` | - | `concurrent` | - | - |
| `PluginsInstall` | `POST` | `/api/plugins/install` | - | `PluginInstallRequest` | `PluginInstallResponse` | - | `concurrent` | - | - |
| `PluginsUpdate` | `POST` | `/api/plugins/update` | - | `PluginUpdateRequest` | `PluginInstallResponse` | - | `concurrent` | - | - |
| `PluginsUninstallById` | `POST` | `/api/plugins/uninstall` | - | `PluginUninstallByIdRequest` | `PluginUninstallResponse` | - | `concurrent` | - | - |
| `PluginsEnable` | `POST` | `/api/plugins/enable` | - | `PluginIdRequest` | `PluginLifecycleResponse` | - | `concurrent` | - | - |
| `PluginsDisable` | `POST` | `/api/plugins/disable` | - | `PluginIdRequest` | `PluginLifecycleResponse` | - | `concurrent` | - | - |
| `PluginsRestart` | `POST` | `/api/plugins/restart` | - | `PluginIdRequest` | `PluginLifecycleResponse` | - | `concurrent` | - | - |
| `PluginsTestConnection` | `POST` | `/api/plugins/{id}/test-connection` | - | - | `PluginTestConnectionResponse` | - | `concurrent` | - | - |
| `PluginsUninstall` | `POST` | `/api/plugins/{id}/uninstall` | - | `PluginUninstallRequest` | `PluginUninstallResponse` | - | `concurrent` | - | - |
| `ChannelsList` | `GET` | `/api/channels` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ChannelsCapabilities` | `GET` | `/api/channels/capabilities` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ChannelsConfig` | `GET` | `/api/channels/config` | - | - | `ChannelsConfigResponse` | - | `concurrent` | - | - |
| `ChannelsConfigUpdate` | `PATCH` | `/api/channels/{provider}/config` | - | `ChannelConfigPatch` | `ChannelConfigSnapshot` | - | `concurrent` | - | - |
| `ChannelsEnable` | `POST` | `/api/channels/{provider}/enable` | - | - | `ChannelConfigSnapshot` | - | `concurrent` | - | - |
| `ChannelsDisable` | `POST` | `/api/channels/{provider}/disable` | - | - | `ChannelConfigSnapshot` | - | `concurrent` | - | - |
| `ChannelsConnect` | `POST` | `/api/channels/{provider}/connect` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ChannelsTest` | `POST` | `/api/channels/{provider}/test` | - | `JsonValue` | `JsonValue` | - | `concurrent` | - | - |
| `GatewayStatus` | `GET` | `/api/gateway/status` | - | - | `GatewayStatusResponse` | - | `concurrent` | - | - |
| `GatewaysList` | `GET` | `/api/gateways` | - | - | `GatewayListResponse` | - | `concurrent` | - | - |
| `GatewayStart` | `POST` | `/api/gateways/{id}/start` | - | `GatewayActionRequest` | `GatewayActionResponse` | - | `concurrent` | - | - |
| `GatewayStop` | `POST` | `/api/gateways/{id}/stop` | - | `GatewayActionRequest` | `GatewayActionResponse` | - | `concurrent` | - | - |
| `ComputerUseStatus` | `GET` | `/api/computer-use/status` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ComputerUsePermissionRequest` | `POST` | `/api/computer-use/permissions/{permission}/request` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ComputerUseTest` | `POST` | `/api/computer-use/test` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `AppshotsStatus` | `GET` | `/api/appshots/status` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `AppshotsCapture` | `POST` | `/api/appshots/capture` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ChromeRelayStatus` | `GET` | `/api/chrome-relay/status` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ChromeRelayLaunch` | `POST` | `/api/chrome-relay/launch` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ChromeRelayTokenRegenerate` | `POST` | `/api/chrome-relay/token/regenerate` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ActivityRecorderStatus` | `GET` | `/api/activity-recorder/status` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ActivityRecorderSessions` | `GET` | `/api/activity-recorder/sessions` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ActivityRecorderClear` | `POST` | `/api/activity-recorder/clear` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `SettingsApply` | `POST` | `/api/settings` | - | `JsonValue` | `JsonValue` | - | `concurrent` | - | - |
| `SettingsLayers` | `GET` | `/api/settings/layers` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `CommandRun` | `POST` | `/api/command` | - | `JsonValue` | `JsonValue` | - | `concurrent` | - | - |
| `MemoryConfigGet` | `GET` | `/api/memory/config` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `MemoryConfigPatch` | `PATCH` | `/api/memory/config` | - | `JsonValue` | `JsonValue` | - | `concurrent` | - | - |
| `MemoryList` | `GET` | `/api/memory` | - | `MemoryListQuery` | `MemoryListResponse` | - | `concurrent` | - | - |
| `MemoryUpdate` | `PATCH` | `/api/memory/{id}` | - | `MemoryUpdateRequest` | `MemoryUpdateResponse` | - | `per-process` | `NotFound` | - |
| `MemoryDreamList` | `GET` | `/api/memory/dream` | - | `MemoryDreamListQuery` | `MemoryDreamListResponse` | - | `concurrent` | - | - |
| `MemoryDreamDetail` | `GET` | `/api/memory/dream/{date}` | - | - | `MemoryDreamDetailResponse` | - | `concurrent` | `NotFound` | - |
| `MemoryProposalList` | `GET` | `/api/memory/proposals` | - | `MemoryProposalListQuery` | `MemoryProposalListResponse` | - | `concurrent` | - | - |
| `MemoryProposalDetail` | `GET` | `/api/memory/proposals/{id}` | - | - | `MemoryProposalDetailResponse` | - | `concurrent` | `NotFound` | - |
| `MemoryProposalApprove` | `POST` | `/api/memory/proposals/{id}/approve` | - | - | `MemoryProposalDecisionResponse` | - | `per-process` | `NotFound`, `Conflict` | - |
| `MemoryProposalReject` | `POST` | `/api/memory/proposals/{id}/reject` | - | - | `MemoryProposalDecisionResponse` | - | `per-process` | `NotFound`, `Conflict` | - |
| `SpeechModels` | `GET` | `/api/speech/models` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `SpeechModelDownload` | `POST` | `/api/speech/models/download` | - | `JsonValue` | `JsonValue` | - | `concurrent` | - | - |
| `SpeechModelDelete` | `DELETE` | `/api/speech/models/{id}` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `SearchCookiesExport` | `POST` | `/api/search/cookies/export` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `SearchCookiesImport` | `POST` | `/api/search/cookies/import` | - | `JsonValue` | `JsonValue` | - | `concurrent` | - | - |
| `SearchCookiesClear` | `POST` | `/api/search/cookies/clear` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `DataExport` | `POST` | `/api/data/export` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `DataImport` | `POST` | `/api/data/import` | - | `JsonValue` | `JsonValue` | - | `concurrent` | - | - |
| `TokenSavings` | `GET` | `/api/token-savings` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `DebugState` | `GET` | `/api/debug/state` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `DebugSessionTrace` | `GET` | `/api/debug/sessions/{id}/trace` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `DebugAction` | `POST` | `/api/debug/actions/{*action}` | - | `JsonValue` | `JsonValue` | - | `concurrent` | - | - |
| `DiscoverySearch` | `GET` | `/api/discovery/search` | - | `DiscoverySearchQuery` | `DiscoverySearchResponse` | - | `concurrent` | `BadRequest`, `Internal` | - |
| `ProtocolRoutes` | `GET` | `/api/-/routes` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `WorkspacesList` | `GET` | `/api/workspaces` | - | - | `WorkspacesResponse` | - | `concurrent` | - | - |
| `WorkspacePatch` | `PATCH` | `/api/workspaces/{workspace_key}` | - | `WorkspacePatchRequest` | `WorkspaceMutationResponse` | - | `concurrent` | - | - |
| `WorkspaceOpen` | `POST` | `/api/workspaces/{workspace_key}/open` | - | `WorkspaceOpenRequest` | `WorkspaceMutationResponse` | - | `concurrent` | - | - |
| `WorkspaceSessionsArchive` | `POST` | `/api/workspaces/{workspace_key}/sessions/archive` | - | `WorkspaceArchiveRequest` | `WorkspaceArchiveResponse` | - | `concurrent` | - | - |
| `AuthStatus` | `GET` | `/api/auth/status` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `AuthLogin` | `POST` | `/api/auth/login` | - | `JsonValue` | `JsonValue` | - | `concurrent` | - | - |
| `AuthLogout` | `POST` | `/api/auth/logout` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `AuthRefresh` | `POST` | `/api/auth/refresh` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `AccountAuthLoginStart` | `POST` | `/api/account-auth/login/start` | - | `AccountLoginStartRequest` | `AccountLoginStartResponse` | - | `concurrent` | - | - |
| `AccountAuthLoginComplete` | `POST` | `/api/account-auth/login/complete` | - | `AccountLoginCompleteRequest` | `AccountAuthStatusResponse` | - | `concurrent` | - | - |
| `AccountAuthStatus` | `GET` | `/api/account-auth/status` | - | - | `AccountAuthStatusResponse` | - | `concurrent` | - | - |
| `AccountAuthRefresh` | `POST` | `/api/account-auth/refresh` | - | - | `AccountAuthStatusResponse` | - | `concurrent` | - | - |
| `AccountAuthLogout` | `POST` | `/api/account-auth/logout` | - | - | `AccountAuthLogoutResponse` | - | `concurrent` | - | - |
| `AccountAuthBilling` | `GET` | `/api/account-auth/billing` | - | - | `AccountBillingSnapshotResponse` | - | `concurrent` | - | - |
| `AccountAuthBillingLedger` | `GET` | `/api/account-auth/billing/ledger` | - | `AccountBillingLedgerQuery` | `AccountBillingLedgerResponse` | - | `concurrent` | - | - |
| `AccountAuthBillingOrder` | `GET` | `/api/account-auth/billing/orders/{id}` | - | `AccountBillingOrderParams` | `AccountBillingOrderResponse` | - | `concurrent` | - | - |
| `ProfilesList` | `GET` | `/api/profiles` | - | - | `ProfileListResponse` | - | `concurrent` | - | - |
| `ProfilesCreate` | `POST` | `/api/profiles` | - | `ProfileCreateRequest` | `JsonValue` | - | `concurrent` | - | - |
| `ProfilesImport` | `POST` | `/api/profiles/import` | - | `ProfileImportRequest` | `JsonValue` | - | `concurrent` | - | - |
| `ProfilesDetail` | `GET` | `/api/profiles/{id}` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ProfilesUpdate` | `PATCH` | `/api/profiles/{id}` | - | `ProfileUpdateRequest` | `JsonValue` | - | `concurrent` | - | - |
| `ProfilesDelete` | `DELETE` | `/api/profiles/{id}` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ProfilesSwitch` | `POST` | `/api/profiles/{id}/switch` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ProfilesExport` | `GET` | `/api/profiles/{id}/export` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ProvidersList` | `GET` | `/api/providers` | - | - | `ProviderListResponse` | - | `concurrent` | - | - |
| `ProvidersCreate` | `POST` | `/api/providers` | - | `ProviderCreateRequest` | `JsonValue` | - | `concurrent` | - | - |
| `ProvidersDetail` | `GET` | `/api/providers/{id}` | - | - | `ProviderDetailResponse` | - | `concurrent` | - | - |
| `ProvidersReplace` | `PUT` | `/api/providers/{id}` | - | `ProviderReplaceRequest` | `ProviderDetailResponse` | - | `concurrent` | - | - |
| `ProvidersOpenaiCodexLocalStatus` | `GET` | `/api/providers/openai-codex/local-status` | - | - | `CodexLocalStatusResponse` | - | `concurrent` | - | - |
| `ProvidersOpenaiCodexApplyLocal` | `POST` | `/api/providers/openai-codex/apply-local` | - | - | `CodexApplyLocalResponse` | - | `concurrent` | - | - |
| `ProvidersUpdate` | `PATCH` | `/api/providers/{id}` | - | `ProviderUpdateRequest` | `JsonValue` | - | `concurrent` | - | - |
| `ProvidersDelete` | `DELETE` | `/api/providers/{id}` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ProvidersProbe` | `POST` | `/api/providers/{id}/probe` | - | `ProviderProbeRequest` | `ProviderProbeResponse` | - | `concurrent` | - | - |
| `ProvidersRefreshModels` | `POST` | `/api/providers/{id}/models/refresh` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `ModelsList` | `GET` | `/api/models` | - | - | `ModelRegistryResponse` | - | `concurrent` | - | - |
| `ModelsUpdate` | `PATCH` | `/api/models/{id}` | - | `ModelUpdateRequest` | `JsonValue` | - | `concurrent` | - | - |
| `ModelsSetDefault` | `POST` | `/api/models/default` | - | `SetDefaultModelRequest` | `JsonValue` | - | `concurrent` | - | - |
| `Credentials` | `GET` | `/api/credentials` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `OAuthStart` | `POST` | `/api/oauth/{provider}/start` | - | `JsonValue` | `JsonValue` | - | `concurrent` | - | - |
| `OAuthPoll` | `POST` | `/api/oauth/{provider}/poll` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `Logs` | `GET` | `/api/logs` | - | `LogsQuery` | `LogsResponse` | - | `concurrent` | - | - |
| `LogsExport` | `GET` | `/api/logs/export` | - | `LogsExportQuery` | `JsonValue` | - | `concurrent` | - | - |
| `DiagnosticsSnapshot` | `GET` | `/api/diagnostics/snapshot` | - | `DiagnosticsQuery` | `DiagnosticsSnapshot` | - | `concurrent` | - | - |
| `DiagnosticsTraces` | `GET` | `/api/diagnostics/traces` | - | `TracesQuery` | `TracesResponse` | - | `concurrent` | - | - |
| `TerminalHealth` | `GET` | `/api/terminal/healthz` | - | - | `TerminalHealthResponse` | - | `concurrent` | - | - |
| `TerminalProfiles` | `GET` | `/api/terminal/profiles` | - | - | `TerminalProfilesResponse` | - | `concurrent` | - | - |
| `TerminalSessionsList` | `GET` | `/api/terminal/sessions` | - | - | `TerminalSessionsResponse` | - | `concurrent` | - | - |
| `TerminalSessionsCreate` | `POST` | `/api/terminal/sessions` | - | `TerminalCreateRequest` | `TerminalSessionSnapshot` | - | `concurrent` | - | - |
| `TerminalSessionDetail` | `GET` | `/api/terminal/sessions/{id}` | - | `TerminalSessionParams` | `TerminalSessionSnapshot` | - | `concurrent` | - | - |
| `TerminalSessionOutput` | `GET` | `/api/terminal/sessions/{id}/output` | - | `TerminalOutputQuery` | `TerminalOutputResponse` | - | `concurrent` | - | - |
| `TerminalSessionDelete` | `DELETE` | `/api/terminal/sessions/{id}` | - | `TerminalSessionParams` | `TerminalSessionSnapshot` | - | `concurrent` | - | - |
| `TerminalSessionWs` | `ANY` | `/api/terminal/sessions/{id}/ws` | `websocket` | - | `EmptyResponse` | - | `concurrent` | - | - |
| `TuiWs` | `ANY` | `/api/tui/ws` | `websocket` | - | `EmptyResponse` | - | `concurrent` | - | - |
| `IpcWs` | `ANY` | `/api/ipc/ws` | `websocket` | - | `EmptyResponse` | - | `concurrent` | - | - |
| `GitLog` | `GET` | `/api/git/log` | - | `GitLogParams` | `GitLogResponse` | - | `concurrent` | - | - |
| `GitDiff` | `GET` | `/api/git/diff` | - | `GitDiffParams` | `GitDiffResponse` | - | `concurrent` | - | - |
| `GitMetadata` | `GET` | `/api/git/metadata` | - | `GitMetadataParams` | `GitMetadataResponse` | - | `concurrent` | - | - |
| `GitWorktrees` | `GET` | `/api/git/worktrees` | - | `GitWorktreesParams` | `GitWorktreesResponse` | - | `concurrent` | - | - |
| `WorktreeSessionsList` | `GET` | `/api/worktree-sessions` | - | `WorktreeSessionsQuery` | `WorktreeSessionsResponse` | - | `concurrent` | - | - |
| `WorktreeSessionsCurrent` | `GET` | `/api/worktree-sessions/current` | - | - | `CurrentWorktreeSessionResponse` | - | `concurrent` | - | - |
| `WorktreeSessionsBySession` | `GET` | `/api/worktree-sessions/{session_id}` | - | `WorktreeSessionBySessionParams` | `WorktreeSessionsResponse` | - | `concurrent` | - | - |
| `Proxy` | `GET` | `/api/proxy` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `Usage` | `GET` | `/api/usage` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `FilesTree` | `GET` | `/api/files/tree` | - | `FileTreeQuery` | `FileTreeResponse` | - | `concurrent` | - | - |
| `FilesStat` | `GET` | `/api/files/stat` | - | `FileStatQuery` | `FileStat` | - | `concurrent` | - | - |
| `FilesRead` | `GET` | `/api/files/read` | - | `FileReadQuery` | `FileReadResponse` | - | `concurrent` | - | - |
| `FilesPreview` | `GET` | `/api/files/preview` | - | `FilePreviewQuery` | `FilePreviewResponse` | - | `concurrent` | - | - |
| `FilesMedia` | `GET` | `/api/files/media` | - | `FileMediaQuery` | `EmptyResponse` | - | `concurrent` | - | - |
| `FilesWrite` | `PUT` | `/api/files/write` | - | `FileWriteRequest` | `FileMutationResponse` | - | `per-key:path` | - | - |
| `FilesUpload` | `POST` | `/api/files/upload` | - | `FileUploadRequest` | `FileUploadResponse` | - | `per-key:path` | - | - |
| `FilesDownload` | `GET` | `/api/files/download` | - | `FileDownloadQuery` | `EmptyResponse` | - | `concurrent` | - | - |
| `FilesMkdir` | `POST` | `/api/files/mkdir` | - | `FileMkdirRequest` | `FileMutationResponse` | - | `per-key:path` | - | - |
| `FilesRename` | `POST` | `/api/files/rename` | - | `FileRenameRequest` | `FileMutationResponse` | - | `per-key:source` | - | - |
| `FilesCopy` | `POST` | `/api/files/copy` | - | `FileCopyRequest` | `FileMutationResponse` | - | `per-key:destination` | - | - |
| `FilesMove` | `POST` | `/api/files/move` | - | `FileMoveRequest` | `FileMutationResponse` | - | `per-key:source` | - | - |
| `FilesDelete` | `DELETE` | `/api/files` | - | `FileDeleteRequest` | `FileMutationResponse` | - | `per-key:path` | - | - |
| `SkillsList` | `GET` | `/api/skills` | - | `SkillsListQuery` | `SkillsListResponse` | - | `concurrent` | - | - |
| `SkillProposalsList` | `GET` | `/api/skills/proposals` | - | `SkillProposalListQuery` | `SkillProposalListResponse` | - | `concurrent` | `BadRequest`, `Forbidden`, `PayloadTooLarge`, `ServiceUnavailable` | - |
| `SkillProposalDetail` | `GET` | `/api/skills/proposals/{proposal_id}` | - | `SkillProposalParams` | `SkillProposalDetailResponse` | - | `concurrent` | `BadRequest`, `Forbidden`, `NotFound`, `Validation`, `PayloadTooLarge`, `ServiceUnavailable` | - |
| `SkillProposalDiff` | `GET` | `/api/skills/proposals/{proposal_id}/diff` | - | `SkillProposalParams` | `SkillProposalDiffResponse` | - | `concurrent` | `BadRequest`, `Forbidden`, `NotFound`, `Validation`, `PayloadTooLarge`, `ServiceUnavailable` | - |
| `SkillProposalApprove` | `POST` | `/api/skills/proposals/{proposal_id}/approve` | - | `SkillProposalMutationParams` | `SkillProposalMutationResponse` | - | `per-key:proposal_id` | `BadRequest`, `Forbidden`, `NotFound`, `Conflict`, `Validation`, `PayloadTooLarge`, `ServiceUnavailable` | - |
| `SkillProposalReject` | `POST` | `/api/skills/proposals/{proposal_id}/reject` | - | `SkillProposalMutationParams` | `SkillProposalMutationResponse` | - | `per-key:proposal_id` | `BadRequest`, `Forbidden`, `NotFound`, `Conflict`, `Validation`, `PayloadTooLarge`, `ServiceUnavailable` | - |
| `SkillsDetail` | `GET` | `/api/skills/{id}` | - | - | `SkillDetailResponse` | - | `concurrent` | - | - |
| `SkillsPatch` | `PATCH` | `/api/skills/{id}` | - | `SkillPatchRequest` | `SkillDetailResponse` | - | `concurrent` | - | - |
| `SkillsFiles` | `GET` | `/api/skills/{id}/files` | - | - | `JsonValue` | - | `concurrent` | - | - |
| `KanbanBoards` | `GET` | `/api/kanban/boards` | - | `KanbanQuery` | `KanbanBoardsResponse` | - | `concurrent` | - | - |
| `KanbanBoardDetail` | `GET` | `/api/kanban/boards/{id}` | - | - | `KanbanBoardDetailResponse` | - | `concurrent` | - | - |
| `KanbanTaskCreate` | `POST` | `/api/kanban/tasks` | - | `KanbanTaskCreateRequest` | `KanbanTaskMutationResponse` | - | `per-key:board_id` | - | - |
| `KanbanTaskUpdate` | `PATCH` | `/api/kanban/tasks/{id}` | - | `KanbanTaskUpdateRequest` | `KanbanTaskMutationResponse` | - | `per-process` | - | - |
| `KanbanTaskComment` | `POST` | `/api/kanban/tasks/{id}/comments` | - | `KanbanCommentCreateRequest` | `KanbanTaskMutationResponse` | - | `per-process` | - | - |
| `WorkflowDefinitionsList` | `GET` | `/api/workflows` | - | `WorkflowDefinitionsQuery` | `WorkflowDefinitionsResponse` | - | `concurrent` | `BadRequest`, `Forbidden`, `PayloadTooLarge`, `ServiceUnavailable` | - |
| `WorkflowDefinitionDetail` | `GET` | `/api/workflows/{workflow}` | - | `WorkflowDefinitionParams` | `WorkflowDefinitionDetail` | - | `concurrent` | `BadRequest`, `Forbidden`, `NotFound`, `PayloadTooLarge`, `ServiceUnavailable` | - |
| `WorkflowRunsList` | `GET` | `/api/workflow-runs` | - | `WorkflowRunListQuery` | `WorkflowRunPage` | - | `concurrent` | `BadRequest`, `Forbidden`, `PayloadTooLarge`, `ServiceUnavailable` | - |
| `WorkflowRunStart` | `POST` | `/api/workflows/{workflow}/runs` | - | `WorkflowRunStartParams` | `WorkflowRunResponse` | - | `per-key:workflow` | `BadRequest`, `Forbidden`, `NotFound`, `Conflict`, `PayloadTooLarge`, `ServiceUnavailable` | - |
| `WorkflowRunStatus` | `GET` | `/api/workflow-runs/{run_id}` | - | `WorkflowRunParams` | `WorkflowRunResponse` | - | `concurrent` | `BadRequest`, `Forbidden`, `NotFound`, `PayloadTooLarge`, `ServiceUnavailable` | - |
| `WorkflowRunAdvance` | `POST` | `/api/workflow-runs/{run_id}/advance` | - | `WorkflowRunAdvanceParams` | `WorkflowRunResponse` | - | `per-key:run_id` | `BadRequest`, `Forbidden`, `NotFound`, `Conflict`, `PayloadTooLarge`, `ServiceUnavailable` | - |
| `WorkflowRunCancel` | `POST` | `/api/workflow-runs/{run_id}/cancel` | - | `WorkflowRunCancelParams` | `WorkflowRunResponse` | - | `per-key:run_id` | `BadRequest`, `Forbidden`, `NotFound`, `Conflict`, `PayloadTooLarge`, `ServiceUnavailable` | - |
| `JobsList` | `GET` | `/api/jobs` | - | `JobsQuery` | `JobsListResponse` | - | `concurrent` | `BadRequest`, `PayloadTooLarge` | - |
| `JobsCreate` | `POST` | `/api/jobs` | - | `JobCreateRequest` | `JobMutationResponse` | - | `per-process` | `BadRequest`, `Conflict`, `PayloadTooLarge` | - |
| `JobsUpdate` | `PATCH` | `/api/jobs/{id}` | - | `JobUpdateParams` | `JobMutationResponse` | - | `per-key:id` | `BadRequest`, `NotFound`, `Conflict`, `PayloadTooLarge` | - |
| `JobsDelete` | `DELETE` | `/api/jobs/{id}` | - | `JobDeleteParams` | `JobMutationResponse` | - | `per-key:id` | `BadRequest`, `NotFound`, `Conflict` | - |
| `JobsPause` | `POST` | `/api/jobs/{id}/pause` | - | `JobActionParams` | `JobMutationResponse` | - | `per-key:id` | `BadRequest`, `NotFound`, `Conflict` | - |
| `JobsResume` | `POST` | `/api/jobs/{id}/resume` | - | `JobActionParams` | `JobMutationResponse` | - | `per-key:id` | `BadRequest`, `NotFound`, `Conflict` | - |
| `JobsRun` | `POST` | `/api/jobs/{id}/run` | - | `JobRunParams` | `JobRunResponse` | - | `per-key:id` | `BadRequest`, `NotFound`, `Conflict`, `PayloadTooLarge`, `ServiceUnavailable` | - |
| `CronHistory` | `GET` | `/api/cron/history` | - | `CronHistoryQuery` | `CronHistoryResponse` | - | `concurrent` | `BadRequest`, `PayloadTooLarge` | - |
| `GroupChatRoomsList` | `GET` | `/api/group-chat/rooms` | - | `GroupChatProfileQuery` | `GroupChatRoomsResponse` | - | `concurrent` | - | - |
| `GroupChatRoomCreate` | `POST` | `/api/group-chat/rooms` | - | `GroupChatRoomCreateRequest` | `GroupChatRoomMutationResponse` | - | `per-process` | `BadRequest`, `Conflict`, `PayloadTooLarge`, `ServiceUnavailable` | - |
| `GroupChatRoomDetail` | `GET` | `/api/group-chat/rooms/{id}` | - | `GroupChatRoomParams` | `GroupChatRoomDetailResponse` | - | `concurrent` | `NotFound` | - |
| `GroupChatRoomDelete` | `DELETE` | `/api/group-chat/rooms/{id}` | - | `GroupChatRoomDeleteParams` | `GroupChatRoomMutationResponse` | - | `per-key:id` | `NotFound`, `Conflict` | - |
| `GroupChatRoomClone` | `POST` | `/api/group-chat/rooms/{id}/clone` | - | `GroupChatRoomCloneParams` | `GroupChatRoomMutationResponse` | - | `per-key:id` | `BadRequest`, `NotFound`, `Conflict`, `PayloadTooLarge` | - |
| `GroupChatInvite` | `GET` | `/api/group-chat/rooms/{id}/invite` | - | `GroupChatRoomParams` | `GroupChatInvite` | - | `concurrent` | `NotFound` | - |
| `GroupChatInviteMutation` | `POST` | `/api/group-chat/rooms/{id}/invite` | - | `GroupChatInviteMutationParams` | `GroupChatInviteMutationResponse` | - | `per-key:id` | `BadRequest`, `NotFound`, `Conflict` | - |
| `GroupChatAgentAdd` | `POST` | `/api/group-chat/rooms/{id}/agents` | - | `GroupChatAgentCreateParams` | `GroupChatAgentMutationResponse` | - | `per-key:room_id` | `BadRequest`, `NotFound`, `Conflict` | - |
| `GroupChatAgentUpdate` | `PATCH` | `/api/group-chat/rooms/{room_id}/agents/{agent_id}` | - | `GroupChatAgentUpdateParams` | `GroupChatAgentMutationResponse` | - | `per-key:room_id` | `BadRequest`, `NotFound`, `Conflict` | - |
| `GroupChatAgentDelete` | `DELETE` | `/api/group-chat/rooms/{room_id}/agents/{agent_id}` | - | `GroupChatAgentDeleteParams` | `GroupChatAgentMutationResponse` | - | `per-key:room_id` | `NotFound`, `Conflict` | - |
| `GroupChatMessage` | `POST` | `/api/group-chat/rooms/{id}/messages` | - | `GroupChatMessageSendParams` | `GroupChatMessageResponse` | - | `per-key:id` | `BadRequest`, `NotFound`, `Conflict`, `PayloadTooLarge`, `ServiceUnavailable` | - |
| `GroupChatCompression` | `POST` | `/api/group-chat/rooms/{id}/context-compression` | - | `GroupChatCompressionUpdateParams` | `GroupChatCompressionResponse` | - | `per-key:id` | `BadRequest`, `NotFound`, `Conflict` | - |
| `GroupChatStream` | `GET` | `/api/group-chat/rooms/{id}/stream` | - | `GroupChatStreamQuery` | `GroupChatStreamEnvelope` | `GroupChatStreamEnvelope` | `concurrent` | `NotFound` | - |
| `BackendServices` | `GET` | `/api/backend-services` | - | `BackendServicesQuery` | `BackendServicesResponse` | - | `concurrent` | `BadRequest`, `Internal` | - |
| `BackendServicesSessionsSync` | `POST` | `/api/backend-services/sessions/sync` | - | `BackendServicesSessionSyncRequest` | `ServiceActionResponse` | - | `per-process` | `BadRequest`, `Internal` | - |
| `BackendServicesContextCompressionRun` | `POST` | `/api/backend-services/context-compression/{id}/run` | - | `BackendServicesCompressionRunParams` | `ServiceActionResponse` | - | `per-key:id` | `BadRequest`, `NotFound`, `Conflict`, `Validation`, `ServiceUnavailable` | - |
| `BackendServicesAgentBridgeRetry` | `POST` | `/api/backend-services/agent-bridge/events/{id}/retry` | - | `BackendServicesAgentRetryParams` | `ServiceActionResponse` | - | `per-key:id` | `BadRequest`, `NotFound`, `Conflict`, `Validation`, `ServiceUnavailable` | - |
| `BackendServicesMigrationsRun` | `POST` | `/api/backend-services/migrations/run` | - | `BackendServicesMigrationRequest` | `ServiceActionResponse` | - | `per-process` | `ServiceUnavailable` | - |
| `BackendServicesBackups` | `POST` | `/api/backend-services/backups` | - | `BackendServicesBackupRequest` | `ServiceActionResponse` | - | `per-process` | `ServiceUnavailable` | - |
| `QueueList` | `GET` | `/api/queue` | - | - | `QueueListResponse` | - | `concurrent` | - | - |
| `QueueAdd` | `POST` | `/api/queue` | - | `QueueAddRequest` | `QueueAddResponse` | - | `per-process` | - | - |
| `QueueUpdate` | `PATCH` | `/api/queue/{id}` | - | `QueueUpdateRequest` | `QueueUpdateResponse` | - | `per-process` | - | - |
| `QueueRemove` | `DELETE` | `/api/queue/{id}` | - | - | `QueueRemoveResponse` | - | `per-process` | - | - |
| `QueueSendNow` | `POST` | `/api/queue/{id}/send-now` | - | - | `QueueSendNowResponse` | - | `per-process` | - | - |
| `TaskList` | `GET` | `/api/tasks` | - | - | `TaskListResponse` | - | `concurrent` | - | - |
| `TaskDetail` | `GET` | `/api/tasks/{id}` | - | `TaskDetailParams` | `TaskDetailResponse` | - | `concurrent` | `NotFound` | - |
| `AgentRuntimeDashboard` | `GET` | `/api/agent-runtime/dashboard` | - | `AgentRuntimeDashboardQuery` | `AgentRuntimeDashboardResponse` | - | `concurrent` | - | - |
| `MentionAutocomplete` | `GET` | `/api/mentions/autocomplete` | - | `MentionAutocompleteQuery` | `MentionAutocompleteResponse` | - | `concurrent` | - | - |
| `SidebarPin` | `POST` | `/api/sidebar/pin` | - | `SidebarPinRequest` | `SidebarPinResponse` | - | `per-process` | - | - |
| `SidebarUnpin` | `POST` | `/api/sidebar/unpin` | - | `SidebarUnpinRequest` | `SidebarUnpinResponse` | - | `per-process` | - | - |
| `SidebarReorder` | `POST` | `/api/sidebar/reorder` | - | `SidebarReorderRequest` | `SidebarReorderResponse` | - | `per-process` | - | - |
| `SessionListAllProfiles` | `GET` | `/api/sessions/all-profiles` | - | - | `SessionListResponse` | - | `concurrent` | - | - |
| `MessagingSectionsList` | `GET` | `/api/messaging/sections` | - | `MessagingSectionsQuery` | `MessagingSectionsListResponse` | - | `concurrent` | - | - |
| `MessagingSectionCreate` | `POST` | `/api/messaging/sections` | - | `MessagingSectionCreateRequest` | `MessagingSectionCreateResponse` | - | `per-process` | - | - |
| `MessagingSectionUpdate` | `PATCH` | `/api/messaging/sections/{id}` | - | `MessagingSectionUpdateRequest` | `MessagingSectionUpdateResponse` | - | `per-process` | - | - |
| `MessagingSectionDelete` | `DELETE` | `/api/messaging/sections/{id}` | - | - | `MessagingSectionDeleteResponse` | - | `per-process` | - | - |
| `ImageGenerate` | `POST` | `/api/images/generate` | - | `ImageGenerateRequest` | `ImageGenerateResponse` | - | `per-process` | - | - |
| `VoiceProviders` | `GET` | `/api/voice/providers` | - | - | `VoiceProvidersResponse` | - | `concurrent` | - | - |
| `VoiceTts` | `POST` | `/api/voice/tts` | - | `TtsRequest` | `TtsResponse` | - | `per-process` | - | - |
| `VoiceStt` | `POST` | `/api/voice/stt` | - | `SttRequest` | `SttResponse` | - | `per-process` | - | - |
