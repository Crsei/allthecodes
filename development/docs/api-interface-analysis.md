# AlltheCodes 前后端 API 接口全景分析

> 最后更新：2026-06-16  
> 分析范围：allthecodes 仓库（Rust 后端 + React 前端）

---

## 目录

1. [总体架构概览](#1-总体架构概览)
2. [HTTP REST API（allthecodes-web）](#2-http-rest-api-allthecodes-web)
3. [HTTP REST API（allthecodes-daemon）](#3-http-rest-api-allthecodes-daemon)
4. [WebSocket 端点](#4-websocket-端点)
5. [SSE 事件流](#5-sse-事件流)
6. [IPC / JSON-Lines 协议](#6-ipc--json-lines-协议)
7. [Gateway 远程控制 API](#7-gateway-远程控制-api)
8. [MCP 协议](#8-mcp-协议)
9. [Webhook 入口](#9-webhook-入口)
10. [前端 Config/Settings API](#10-前端-configsettings-api)
11. [认证与授权](#11-认证与授权)
12. [前端依赖的端点汇总](#12-前端依赖的端点汇总)

---

## 1. 总体架构概览

```
                    ┌────────────────────────┐
                    │    React 前端 (SPA)     │
                    │  allthecodes-web (TS)   │
                    └──────┬─────────┬───────┘
                           │         │
              ┌────────────┤  WS/WSS │────────────┐
              ▼            │         │            ▼
    ┌─────────────────┐   │         │   ┌───────────────────┐
    │  HTTP REST API  │   │  IPC WS │   │  JSON-RPC WS      │
    │  (/api/*)       │   │(/api/   │   │  (/api/rpc/ws)    │
    │                 │   │ ipc/ws) │   │                   │
    └────────┬────────┘   └────┬────┘   └────────┬──────────┘
             │                 │                  │
             ▼                 ▼                  ▼
    ┌─────────────────────────────────────────────────────┐
    │              allthecodes-web (Axum HTTP Server)      │
    │  端口 17322（默认）                                    │
    │  绑定 127.0.0.1                                      │
    │  handler_registry -> ApiDispatcher -> 各 handler      │
    └────────────────────┬─────────────────────────────────┘
                         │
                         │ IPC / 内部调用
                         ▼
    ┌─────────────────────────────────────────────────────┐
    │              allthecodes-daemon (Axum HTTP Server)   │
    │  端口 19836（默认）                                    │
    │  绑定 127.0.0.1                                      │
    │  - /api/* (submit, abort, command, etc.)            │
    │  - /webhook/*                                       │
    │  - /events (SSE)                                    │
    │  - /health                                          │
    │  - /remote-control/v1/* (Gateway)                   │
    │  - /api/claude_code/team_memory                     │
    └────────────────────┬─────────────────────────────────┘
                         │
                         ▼
    ┌─────────────────────────────────────────────────────┐
    │  JSON-Lines 协议 (stdin/stdout) → 后端引擎            │
    │  IPC v1: FrontendMessage / BackendMessage            │
    │  IPC v2: IpcPayload + IpcEnvelope                    │
    │  子系统: LSP/MCP/Plugin/Skill/IDE/Agent              │
    └─────────────────────────────────────────────────────┘
```

---

## 2. HTTP REST API（allthecodes-web）

**位置**：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/crates/allthecodes-web/src/`  
**框架**：Axum  
**默认端口**：17322  
**绑定地址**：`127.0.0.1`  
**状态**：**外部可访问**（但仅限本地回环）  
**前缀**：所有 REST 端点均以 `/api/*` 开头，同时提供 `/api/v2/*` 版本化镜像

### 2.1 协议元数据

- 序列化格式：JSON
- 序列化策略标签：`Concurrent`（并发读取）、`PerProcess`（进程级串行化）、`PerKey("field")`（基于字段的串行化）
- 每个端点在 `allthecodes-protocol` crate 的 `request.rs` 中通过 `api_definitions!` 宏声明
- 前端通过 `handler_registry.rs` 的 `register_protocol_routes()` 注册
- 静态 SPA 文件通过 `static_files::static_handler` fallback 提供

### 2.2 端点完整列表

| 操作 (ApiMethod) | HTTP | 路径 | 方向 | 请求类型 | 响应类型 | 串行化策略 |
|---|---|---|---|---|---|---|
| **聊天相关** ||||||
| Chat | POST | /api/chat | C→S | ChatRequest | Value | PerKey("session_id") |
| Abort | POST | /api/abort | C→S | AbortRequest | Value | PerKey("session_id") |
| ChatPermissionResponse | POST | /api/chat/permissions/{tool_use_id}/response | C→S | ChatPermissionResponseRequest | Value | PerKey("session_id") |
| State | GET | /api/state | C→S | - | StateResponse | Concurrent |
| SystemPrompt | GET | /api/system-prompt | C→S | - | SystemPromptResponse | Concurrent |
| CodingAgentsStatus | GET | /api/coding-agents/status | C→S | - | Vec<CodingAgentStatus> | Concurrent |
| **启动台** ||||||
| LaunchpadSnapshotCreate | POST | /api/launchpad/snapshots | C→S | LaunchpadSnapshotCreateRequest | LaunchpadSnapshotCreateResponse | PerProcess |
| **会话管理** ||||||
| SessionList | GET | /api/sessions | C→S | - | SessionListResponse | Concurrent |
| SessionCreate | POST | /api/sessions/new | C→S | SessionCreateParams | SessionCreateResponse | PerProcess |
| SessionDetail | GET | /api/sessions/{id} | C→S | SessionDetailParams | SessionDetailResponse | PerKey("id") |
| SessionResume | POST | /api/sessions/{id}/resume | C→S | SessionResumeParams | SessionResumeResponse | PerKey("id") |
| SessionArchive | POST | /api/sessions/{id}/archive | C→S | SessionArchiveParams | SessionArchiveResponse | PerKey("id") |
| SessionModePatch | PATCH | /api/sessions/{id}/mode | C→S | SessionModePatchParams | SessionModePatchResponse | PerKey("id") |
| SessionMessageBranch | POST | /api/sessions/{id}/messages/{message_id}/branch | C→S | SessionMessageActionParams | Value | PerProcess |
| SessionMessageFeedback | POST | /api/sessions/{id}/messages/{message_id}/feedback | C→S | SessionMessageActionParams | Value | PerProcess |
| SessionMessageDelete | POST | /api/sessions/{id}/messages/{message_id}/delete | C→S | SessionMessageActionParams | Value | PerProcess |
| SessionMessageRegeneratePrepare | POST | /api/sessions/{id}/messages/{message_id}/regenerate/prepare | C→S | SessionMessageActionParams | Value | PerProcess |
| SessionMessageEditPrepare | POST | /api/sessions/{id}/messages/{message_id}/edit/prepare | C→S | SessionMessageActionParams | Value | PerProcess |
| SessionMessageRollbackPreview | POST | /api/sessions/{id}/messages/{message_id}/rollback/preview | C→S | SessionMessageActionParams | Value | PerProcess |
| SessionMessageRollback | POST | /api/sessions/{id}/messages/{message_id}/rollback | C→S | SessionMessageActionParams | Value | PerProcess |
| **能力发现** ||||||
| Capabilities | GET | /api/capabilities | C→S | - | CapabilityDiscoveryResponse | Concurrent |
| **聊天模式** ||||||
| ChatModesList | GET | /api/chat-modes | C→S | - | ChatModesResponse | Concurrent |
| ChatModesResources | GET | /api/chat-modes/resources | C→S | - | ChatModeResourcesResponse | Concurrent |
| ChatModesUpsert | PUT | /api/chat-modes/{id} | C→S | ChatModeBundleUpsertRequest | Value | Concurrent |
| ChatModesDelete | DELETE | /api/chat-modes/{id} | C→S | - | Value | Concurrent |
| **Agents 管理** ||||||
| AgentsList | GET | /api/agents | C→S | - | AgentsListResponse | Concurrent |
| AgentsCreate | POST | /api/agents | C→S | AgentUpsertRequest | Value | Concurrent |
| AgentsDetail | GET | /api/agents/{name} | C→S | AgentDetailParams | Value | Concurrent |
| AgentsUpdate | PATCH | /api/agents/{name} | C→S | AgentUpsertRequest | Value | Concurrent |
| AgentsDelete | DELETE | /api/agents/{name} | C→S | AgentDeleteParams | Value | Concurrent |
| AgentsRestore | POST | /api/agents/{name}/restore | C→S | - | AgentRestoreResponse | Concurrent |
| **People 管理** ||||||
| PeopleList | GET | /api/people | C→S | - | PeopleListResponse | Concurrent |
| PeopleCreate | POST | /api/people | C→S | PersonCreateRequest | PersonMutationResponse | Concurrent |
| PeopleDetail | GET | /api/people/{id} | C→S | - | PersonMutationResponse | Concurrent |
| PeopleUpdate | PATCH | /api/people/{id} | C→S | PersonUpdateRequest | PersonMutationResponse | Concurrent |
| PeopleDelete | DELETE | /api/people/{id} | C→S | - | PersonMutationResponse | Concurrent |
| **Hooks 管理** ||||||
| HooksList | GET | /api/hooks | C→S | - | HooksListResponse | Concurrent |
| HooksCreate | POST | /api/hooks | C→S | HookEventRequest | HookEventResponse | Concurrent |
| HooksTest | POST | /api/hooks/test | C→S | HookEventRequest | HookEventResponse | Concurrent |
| HooksDetail | GET | /api/hooks/{event} | C→S | - | HookEventResponse | Concurrent |
| HooksUpdate | PATCH | /api/hooks/{event} | C→S | HookEventUpdateRequest | HookEventResponse | Concurrent |
| HooksDelete | DELETE | /api/hooks/{event} | C→S | - | HookEventResponse | Concurrent |
| **Prompts 管理** ||||||
| PromptsList | GET | /api/prompts | C→S | - | PromptsListResponse | Concurrent |
| PromptsCreate | POST | /api/prompts | C→S | PromptCreateRequest | PromptMutationResponse | Concurrent |
| PromptsDetail | GET | /api/prompts/{id} | C→S | - | PromptMutationResponse | Concurrent |
| PromptsUpdate | PATCH | /api/prompts/{id} | C→S | PromptUpdateRequest | PromptMutationResponse | Concurrent |
| PromptsDelete | DELETE | /api/prompts/{id} | C→S | - | PromptMutationResponse | Concurrent |
| **MCP 服务器管理** ||||||
| McpServersList | GET | /api/mcp-servers | C→S | - | Value | Concurrent |
| McpServersCreate | POST | /api/mcp-servers | C→S | Value | Value | Concurrent |
| McpServersMarketplace | GET | /api/mcp-servers/marketplace | C→S | - | Value | Concurrent |
| McpServersDetail | GET | /api/mcp-servers/{name} | C→S | - | Value | Concurrent |
| McpServersUpdate | PATCH | /api/mcp-servers/{name} | C→S | Value | Value | Concurrent |
| McpServersDelete | DELETE | /api/mcp-servers/{name} | C→S | - | Value | Concurrent |
| **插件管理** ||||||
| PluginsList | GET | /api/plugins | C→S | - | PluginsListResponse | Concurrent |
| PluginsMarketplace | GET | /api/plugins/marketplace | C→S | - | PluginsMarketplaceResponse | Concurrent |
| PluginsInstall | POST | /api/plugins/install | C→S | PluginInstallRequest | PluginInstallResponse | Concurrent |
| PluginsUninstall | POST | /api/plugins/{id}/uninstall | C→S | PluginUninstallRequest | PluginUninstallResponse | Concurrent |
| **渠道** ||||||
| ChannelsList | GET | /api/channels | C→S | - | Value | Concurrent |
| ChannelsCapabilities | GET | /api/channels/capabilities | C→S | - | Value | Concurrent |
| ChannelsConnect | POST | /api/channels/{provider}/connect | C→S | - | Value | Concurrent |
| ChannelsTest | POST | /api/channels/{provider}/test | C→S | Value | Value | Concurrent |
| **Gateway** ||||||
| GatewayStatus | GET | /api/gateway/status | C→S | - | GatewayStatusResponse | Concurrent |
| GatewaysList | GET | /api/gateways | C→S | - | GatewayListResponse | Concurrent |
| GatewayStart | POST | /api/gateways/{id}/start | C→S | GatewayActionRequest | GatewayActionResponse | Concurrent |
| GatewayStop | POST | /api/gateways/{id}/stop | C→S | GatewayActionRequest | GatewayActionResponse | Concurrent |
| **集成服务** ||||||
| ComputerUseStatus | GET | /api/computer-use/status | C→S | - | Value | Concurrent |
| ComputerUsePermissionRequest | POST | /api/computer-use/permissions/{permission}/request | C→S | - | Value | Concurrent |
| ComputerUseTest | POST | /api/computer-use/test | C→S | - | Value | Concurrent |
| AppshotsStatus | GET | /api/appshots/status | C→S | - | Value | Concurrent |
| AppshotsCapture | POST | /api/appshots/capture | C→S | - | Value | Concurrent |
| ChromeRelayStatus | GET | /api/chrome-relay/status | C→S | - | Value | Concurrent |
| ChromeRelayLaunch | POST | /api/chrome-relay/launch | C→S | - | Value | Concurrent |
| ChromeRelayTokenRegenerate | POST | /api/chrome-relay/token/regenerate | C→S | - | Value | Concurrent |
| ActivityRecorderStatus | GET | /api/activity-recorder/status | C→S | - | Value | Concurrent |
| ActivityRecorderSessions | GET | /api/activity-recorder/sessions | C→S | - | Value | Concurrent |
| ActivityRecorderClear | POST | /api/activity-recorder/clear | C→S | - | Value | Concurrent |
| **设置与调试** ||||||
| SettingsApply | POST | /api/settings | C→S | Value | Value | Concurrent |
| CommandRun | POST | /api/command | C→S | Value | Value | Concurrent |
| MemoryConfigGet | GET | /api/memory/config | C→S | - | Value | Concurrent |
| MemoryConfigPatch | PATCH | /api/memory/config | C→S | Value | Value | Concurrent |
| MemoryList | GET | /api/memory | C→S | - | Value | Concurrent |
| MemoryUpdate | PATCH | /api/memory/{id} | C→S | Value | Value | Concurrent |
| SpeechModels | GET | /api/speech/models | C→S | - | Value | Concurrent |
| SpeechModelDownload | POST | /api/speech/models/download | C→S | Value | Value | Concurrent |
| SpeechModelDelete | DELETE | /api/speech/models/{id} | C→S | - | Value | Concurrent |
| SearchCookiesExport | POST | /api/search/cookies/export | C→S | - | Value | Concurrent |
| SearchCookiesImport | POST | /api/search/cookies/import | C→S | Value | Value | Concurrent |
| SearchCookiesClear | POST | /api/search/cookies/clear | C→S | - | Value | Concurrent |
| DataExport | POST | /api/data/export | C→S | - | Value | Concurrent |
| DataImport | POST | /api/data/import | C→S | Value | Value | Concurrent |
| TokenSavings | GET | /api/token-savings | C→S | - | Value | Concurrent |
| DebugState | GET | /api/debug/state | C→S | - | Value | Concurrent |
| DebugSessionTrace | GET | /api/debug/sessions/{id}/trace | C→S | - | Value | Concurrent |
| DebugAction | POST | /api/debug/actions/{*action} | C→S | Value | Value | Concurrent |
| ProtocolRoutes | GET | /api/-/routes | C→S | - | Value | Concurrent |
| **工作区管理** ||||||
| WorkspacesList | GET | /api/workspaces | C→S | - | WorkspacesResponse | Concurrent |
| WorkspacePatch | PATCH | /api/workspaces/{workspace_key} | C→S | WorkspacePatchRequest | WorkspaceMutationResponse | Concurrent |
| WorkspaceOpen | POST | /api/workspaces/{workspace_key}/open | C→S | WorkspaceOpenRequest | WorkspaceMutationResponse | Concurrent |
| WorkspaceSessionsArchive | POST | /api/workspaces/{workspace_key}/sessions/archive | C→S | WorkspaceArchiveRequest | WorkspaceArchiveResponse | Concurrent |
| **认证** ||||||
| AuthStatus | GET | /api/auth/status | C→S | - | Value | Concurrent |
| AuthLogin | POST | /api/auth/login | C→S | Value | Value | Concurrent |
| AuthLogout | POST | /api/auth/logout | C→S | - | Value | Concurrent |
| AuthRefresh | POST | /api/auth/refresh | C→S | - | Value | Concurrent |
| **Profile 管理** ||||||
| ProfilesList | GET | /api/profiles | C→S | - | ProfileListResponse | Concurrent |
| ProfilesCreate | POST | /api/profiles | C→S | ProfileCreateRequest | Value | Concurrent |
| ProfilesImport | POST | /api/profiles/import | C→S | ProfileImportRequest | Value | Concurrent |
| ProfilesDetail | GET | /api/profiles/{id} | C→S | - | Value | Concurrent |
| ProfilesUpdate | PATCH | /api/profiles/{id} | C→S | ProfileUpdateRequest | Value | Concurrent |
| ProfilesDelete | DELETE | /api/profiles/{id} | C→S | - | Value | Concurrent |
| ProfilesSwitch | POST | /api/profiles/{id}/switch | C→S | - | Value | Concurrent |
| ProfilesExport | GET | /api/profiles/{id}/export | C→S | - | Value | Concurrent |
| **Provider 管理** ||||||
| ProvidersList | GET | /api/providers | C→S | - | ProviderListResponse | Concurrent |
| ProvidersCreate | POST | /api/providers | C→S | ProviderCreateRequest | Value | Concurrent |
| ProvidersOpenaiCodexLocalStatus | GET | /api/providers/openai-codex/local-status | C→S | - | CodexLocalStatusResponse | Concurrent |
| ProvidersOpenaiCodexApplyLocal | POST | /api/providers/openai-codex/apply-local | C→S | - | CodexApplyLocalResponse | Concurrent |
| ProvidersUpdate | PATCH | /api/providers/{id} | C→S | ProviderUpdateRequest | Value | Concurrent |
| ProvidersDelete | DELETE | /api/providers/{id} | C→S | - | Value | Concurrent |
| ProvidersRefreshModels | POST | /api/providers/{id}/models/refresh | C→S | - | Value | Concurrent |
| **模型管理** ||||||
| ModelsList | GET | /api/models | C→S | - | ModelRegistryResponse | Concurrent |
| ModelsUpdate | PATCH | /api/models/{id} | C→S | ModelUpdateRequest | Value | Concurrent |
| ModelsSetDefault | POST | /api/models/default | C→S | SetDefaultModelRequest | Value | Concurrent |
| **凭据** ||||||
| Credentials | GET | /api/credentials | C→S | - | Value | Concurrent |
| OAuthStart | POST | /api/oauth/{provider}/start | C→S | Value | Value | Concurrent |
| OAuthPoll | POST | /api/oauth/{provider}/poll | C→S | - | Value | Concurrent |
| **日志与诊断** ||||||
| Logs | GET | /api/logs | C→S | - | Value | Concurrent |
| LogsExport | GET | /api/logs/export | C→S | - | Value | Concurrent |
| DiagnosticsSnapshot | GET | /api/diagnostics/snapshot | C→S | - | Value | Concurrent |
| DiagnosticsTraces | GET | /api/diagnostics/traces | C→S | - | Value | Concurrent |
| **终端** ||||||
| TerminalProfiles | GET | /api/terminal/profiles | C→S | - | EmptyResponse | Concurrent |
| TerminalSessionsList | GET | /api/terminal/sessions | C→S | - | EmptyResponse | Concurrent |
| TerminalSessionsCreate | POST | /api/terminal/sessions | C→S | - | EmptyResponse | Concurrent |
| TerminalSessionDetail | GET | /api/terminal/sessions/{id} | C→S | - | EmptyResponse | Concurrent |
| TerminalSessionDelete | DELETE | /api/terminal/sessions/{id} | C→S | - | EmptyResponse | Concurrent |
| TerminalSessionWs | ANY | /api/terminal/sessions/{id}/ws | (UPGRADE) | - | EmptyResponse | - |
| TuiWs | ANY | /api/tui/ws | (UPGRADE) | - | EmptyResponse | - |
| IpcWs | ANY | /api/ipc/ws | (UPGRADE) | - | EmptyResponse | - |
| **侧边栏工具** ||||||
| GitLog | GET | /api/git/log | C→S | - | Value | Concurrent |
| GitDiff | GET | /api/git/diff | C→S | - | Value | Concurrent |
| Proxy | GET | /api/proxy | C→S | - | Value | Concurrent |
| Usage | GET | /api/usage | C→S | - | Value | Concurrent |
| **文件操作** ||||||
| FilesTree | GET | /api/files/tree | C→S | FileTreeQuery | FileTreeResponse | Concurrent |
| FilesStat | GET | /api/files/stat | C→S | FileStatQuery | FileStat | Concurrent |
| FilesRead | GET | /api/files/read | C→S | FileReadQuery | FileReadResponse | Concurrent |
| FilesWrite | PUT | /api/files/write | C→S | FileWriteRequest | FileMutationResponse | PerKey("path") |
| FilesUpload | POST | /api/files/upload | C→S | FileUploadRequest | FileUploadResponse | PerKey("path") |
| FilesDownload | GET | /api/files/download | C→S | FileDownloadQuery | EmptyResponse | Concurrent |
| FilesMkdir | POST | /api/files/mkdir | C→S | FileMkdirRequest | FileMutationResponse | PerKey("path") |
| FilesRename | POST | /api/files/rename | C→S | FileRenameRequest | FileMutationResponse | PerKey("source") |
| FilesCopy | POST | /api/files/copy | C→S | FileCopyRequest | FileMutationResponse | PerKey("destination") |
| FilesMove | POST | /api/files/move | C→S | FileMoveRequest | FileMutationResponse | PerKey("source") |
| FilesDelete | DELETE | /api/files | C→S | FileDeleteRequest | FileMutationResponse | PerKey("path") |
| **技能管理** ||||||
| SkillsList | GET | /api/skills | C→S | SkillsListQuery | SkillsListResponse | Concurrent |
| SkillsDetail | GET | /api/skills/{id} | C→S | - | SkillDetailResponse | Concurrent |
| SkillsPatch | PATCH | /api/skills/{id} | C→S | SkillPatchRequest | SkillDetailResponse | Concurrent |
| SkillsFiles | GET | /api/skills/{id}/files | C→S | - | Value | Concurrent |
| **看板** ||||||
| KanbanBoards | GET | /api/kanban/boards | C→S | KanbanQuery | KanbanBoardsResponse | Concurrent |
| KanbanBoardDetail | GET | /api/kanban/boards/{id} | C→S | - | KanbanBoardDetailResponse | Concurrent |
| KanbanTaskCreate | POST | /api/kanban/tasks | C→S | KanbanTaskCreateRequest | KanbanTaskMutationResponse | PerKey("board_id") |
| KanbanTaskUpdate | PATCH | /api/kanban/tasks/{id} | C→S | KanbanTaskUpdateRequest | KanbanTaskMutationResponse | PerProcess |
| KanbanTaskComment | POST | /api/kanban/tasks/{id}/comments | C→S | KanbanCommentCreateRequest | KanbanTaskMutationResponse | PerProcess |
| **定时任务** ||||||
| JobsList | GET | /api/jobs | C→S | - | Value | Concurrent |
| JobsCreate | POST | /api/jobs | C→S | Value | Value | Concurrent |
| JobsUpdate | PATCH | /api/jobs/{id} | C→S | Value | Value | Concurrent |
| JobsDelete | DELETE | /api/jobs/{id} | C→S | - | Value | Concurrent |
| JobsPause | POST | /api/jobs/{id}/pause | C→S | - | Value | Concurrent |
| JobsResume | POST | /api/jobs/{id}/resume | C→S | - | Value | Concurrent |
| JobsRun | POST | /api/jobs/{id}/run | C→S | - | Value | Concurrent |
| CronHistory | GET | /api/cron/history | C→S | - | Value | Concurrent |
| **群聊** ||||||
| GroupChatRoomsList | GET | /api/group-chat/rooms | C→S | - | Value | Concurrent |
| GroupChatRoomCreate | POST | /api/group-chat/rooms | C→S | Value | Value | Concurrent |
| GroupChatRoomDetail | GET | /api/group-chat/rooms/{id} | C→S | - | Value | Concurrent |
| GroupChatRoomDelete | DELETE | /api/group-chat/rooms/{id} | C→S | - | Value | Concurrent |
| GroupChatRoomClone | POST | /api/group-chat/rooms/{id}/clone | C→S | - | Value | Concurrent |
| GroupChatInvite | GET | /api/group-chat/rooms/{id}/invite | C→S | - | Value | Concurrent |
| GroupChatAgentAdd | POST | /api/group-chat/rooms/{id}/agents | C→S | Value | Value | Concurrent |
| GroupChatAgentUpdate | PATCH | /api/group-chat/rooms/{room_id}/agents/{agent_id} | C→S | Value | Value | Concurrent |
| GroupChatAgentDelete | DELETE | /api/group-chat/rooms/{room_id}/agents/{agent_id} | C→S | - | Value | Concurrent |
| GroupChatMessage | POST | /api/group-chat/rooms/{id}/messages | C→S | Value | Value | Concurrent |
| GroupChatCompression | POST | /api/group-chat/rooms/{id}/context-compression | C→S | - | Value | Concurrent |
| GroupChatStream | GET | /api/group-chat/rooms/{id}/stream | C→S | - | Value | Concurrent |
| **后端服务** ||||||
| BackendServices | GET | /api/backend-services | C→S | - | Value | Concurrent |
| BackendServicesSessionsSync | POST | /api/backend-services/sessions/sync | C→S | - | Value | Concurrent |
| BackendServicesContextCompressionRun | POST | /api/backend-services/context-compression/{id}/run | C→S | - | Value | Concurrent |
| BackendServicesAgentBridgeRetry | POST | /api/backend-services/agent-bridge/events/{id}/retry | C→S | - | Value | Concurrent |
| BackendServicesMigrationsRun | POST | /api/backend-services/migrations/run | C→S | - | Value | Concurrent |
| BackendServicesBackups | POST | /api/backend-services/backups | C→S | Value | Value | Concurrent |
| **健康检查** ||||||
| Health | GET | /api/healthz | C→S | - | HealthResponse | Concurrent |

**注**：所有端点都自动注册 `/api/v2/*` 版本化路径（如 `/api/v2/sessions` 对应 `/api/sessions`）。

### 2.3 前端 Web UI 专用端点

**位置**：`web_state_routes.rs`

| 路径 | 方法 | 用途 |
|---|---|---|
| /api/web/health | GET | Web UI 存储健康检查 |
| /api/web/preferences/fields | GET | 获取首选项字段 |
| /api/web/preferences | GET/PUT | 获取/更新首选项 |
| /api/web/themes | GET/POST | 列出/创建主题方案 |
| /api/web/themes/{id} | PUT/DELETE | 更新/删除主题方案 |
| /api/web/prompts | GET/POST | 列出/创建提示词 |
| /api/web/prompts/{id} | PUT/DELETE | 更新/删除提示词 |
| /api/web/layouts | GET/POST | 列出/创建工作区布局 |
| /api/web/layouts/{id} | PUT/DELETE | 更新/删除布局 |
| /api/web/layouts/{id}/set-default | POST | 设置默认布局 |

这些端点也提供 `/api/v2/web/*` 镜像。

### 2.4 错误响应格式

```json
{
  "error": "错误描述",
  "code": "错误码字符串"
}
```

HTTP 状态码：400 Bad Request、404 Not Found、500 Internal Server Error 等。

---

## 3. HTTP REST API（allthecodes-daemon）

**位置**：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/crates/allthecodes-daemon/src/`  
**框架**：Axum  
**默认端口**：19836  
**绑定地址**：`127.0.0.1`  
**状态**：**内部接口**（仅本地回环，需 daemon 控制令牌认证）

### 3.1 端点列表

#### 3.1.1 API 端点 (`/api/*`)

**认证**：通过 `x-allthecodes-daemon-token` 头或 `Authorization: Bearer <token>` 头（`require_control_token`）

| 路径 | 方法 | 用途 | 方向 | 请求体 | 响应体 |
|---|---|---|---|---|---|
| /api/submit | POST | 提交用户消息 | C→S | `{text, id?, idempotency_key?}` | `{status, message_id, command_id}` |
| /api/abort | POST | 中止当前查询 | C→S | - | `{status, command_id}` |
| /api/command | POST | 执行斜杠命令 | C→S | `{raw}` | `{status, kind, ...}` |
| /api/permission | POST | 回复权限请求 | C→S | `{tool_use_id, decision}` | `{status, command_id}` |
| /api/status | GET | 获取 daemon 状态 | C→S | - | `StatusResponse`（详见下文） |
| /api/attach | POST | 客户端重新连接 | C→S | `{client_id, last_seen_event?}` | `{status, missed_events}` |
| /api/detach | POST | 客户端断开连接 | C→S | `{client_id}` | `{status}` |
| /api/resize | POST | 终端大小变化通知 | C→S | -（当前为 noop stub，未转发 resize 事件） | `{status, message}` |
| /api/history | GET | 获取对话历史 | C→S | - | `{history, sse_events, daemon_events}` |

#### `/api/resize` 当前状态

`POST /api/resize` 目前只校验 daemon 控制令牌并返回 `{"status":"noop","message":"resize forwarding is not implemented yet"}`。它尚未把 resize 事件转发到 Rust TUI、terminal session 或 daemon client metadata；具体 wire shape 留给后续 Phase 决策。

**StatusResponse 字段**：
```json
{
  "kairos_active": bool,
  "proactive": bool,
  "query_running": bool,
  "clients_connected": int,
  "sleeping": bool,
  "daemon_sleep_until": "RFC3339 string | null",
  "daemon_sleep_reason": "string | null",
  "permission_mode": "string",
  "plan_workflow": "PlanWorkflowRecord | null",
  "supervisor_status": "running|stale|stopped",
  "supervisor_pid": "int | null",
  "health_url": "string | null",
  "workers": ["DaemonWorkerSummary"],
  "command_root": "string",
  "assistant_event_log": "string"
}
```

#### 3.1.2 Webhook 端点 (`/webhook/*`)

| 路径 | 方法 | 用途 | 方向 | 认证 |
|---|---|---|---|---|
| /webhook/github | POST | GitHub Webhook | →S | HMAC-SHA256 (X-Hub-Signature-256) |
| /webhook/slack | POST | Slack Webhook | →S | HMAC-SHA256 (X-Slack-Signature) |
| /webhook/generic | POST | 通用 Webhook | →S | 可选令牌 |
| /remote-control/v1/webhooks/{route_id} | POST | 声明式 Webhook 路由 | →S | 每路由秘密令牌 |

#### 3.1.3 内存代理

| 路径 | 方法 | 用途 |
|---|---|---|
| /api/claude_code/team_memory | GET/PUT | 团队内存代理（启用 `TeamMemory` 特性时） |

#### 3.1.4 健康检查

| 路径 | 方法 | 用途 |
|---|---|---|
| /health | GET | 简单存活探针，返回 `{"status":"ok"}` |

#### 3.1.5 Gateway 路由 (`/remote-control/v1/*`)

详见第 7 节。

### 3.2 SSE 事件流

**路径**：`GET /events?client_id=...&last_event_id=...`

**方向**：server → client  
**状态**：**内部接口**（前端通过 daemon WebSocket 间接消费）

| 事件类型 (event_type) | 含义 | 数据字段 |
|---|---|---|
| daemon_command | 命令派发通知 | `{command_id, worker_id, kind}` |
| daemon_{event_type} | 工作进程事件（由 protocol_store 提供） | `{worker_id, command_id, event_type, data, created_at}` |
| stream_start | AI 流开始 | `{message_id, tools, model, session_id}` |
| stream_delta | 流式增量 | `{message_id, event, session_id}` |
| assistant_message | 完整助手消息 | `{message_id, message, session_id}` |
| user_replay | 用户回放 | `{message_id, content, session_id}` |
| stream_end | 流结束 | `{message_id, subtype, is_error, duration_ms, result, session_id}` |
| tombstone | 消息作废 | `{message_id, assistant_id, session_id}` |
| api_retry | API 重试 | `{message_id, attempt, max_retries, retry_delay_ms, error_status, error, session_id}` |
| compact_boundary | 上下文压缩 | `{message_id, compact_metadata, session_id}` |
| tool_use_summary | 工具使用摘要 | `{message_id, summary, preceding_tool_use_ids, session_id}` |
| goal_updated | 目标更新 | `{message_id, event, goal, session_id}` |
| system_info | 系统信息 | `{text, level}` |
| plan_workflow_event | 计划工作流事件 | `{event, summary, ...}` |

**重新连接机制**：客户端传入 `last_event_id`，服务端重放未接收事件（最多缓存 1000 条）。

---

## 4. WebSocket 端点

### 4.1 IPC WebSocket (`/api/ipc/ws`)

- **路径**：`ANY /api/ipc/ws`
- **方向**：双向
- **状态**：**外部可访问**（前端主要通信通道）
- **查询参数**：`?session_id=...`（可选，指定会话）

**前端 → 后端**（`FrontendMessage`，JSON 文本帧）：

| type | 字段 | 说明 |
|---|---|---|
| submit_prompt | text, id | 提交用户提示 |
| abort_query | - | 中止当前查询 |
| permission_response | tool_use_id, decision, feedback?, session_id?, turn_id? | 回复权限请求 |
| question_response | id, text, session_id?, turn_id? | 回复问题 |
| slash_command | raw | 执行斜杠命令 |
| resize | cols, rows | 终端大小变化 |
| quit | - | 退出 |
| query_subsystem_status | - | 查询子系统状态 |
| request_completions | input, cursor_pos, request_id | 请求补全 |
| agent_command/team_command | command | Agent/团队命令 |
| lsp_command/mcp_command/plugin_command/skill_command/ide_command/agent_settings_command | command | 子系统命令 |
| search_files | request_id, pattern, ... | 文件搜索 |
| install_recommended_plugin | plugin_id | 安装推荐插件 |

**后端 → 前端**（`BackendMessage`，JSON 文本帧）：

| type | 说明 |
|---|---|
| ready | 后端就绪，含 session_id, model, cwd, permission_mode, available_models |
| stream_start | AI 内容块开始流式输出 |
| stream_delta | 流式文本增量 |
| thinking_delta | 思考过程增量 |
| stream_end | 内容块流结束 |
| assistant_message | 完整助手消息 |
| tombstone | 消息作废 |
| tool_use | 工具调用 |
| tool_result | 工具结果 |
| tool_progress | 工具进度更新 |
| permission_request | 权限请求 |
| question_request | 问题请求 |
| system_info | 系统信息 |
| usage_update | 用量更新 |
| error | 错误（含 recoverable 标记） |
| goal_updated | 目标更新 |
| status_line_update | 状态行更新 |
| suggestions | 建议列表 |
| file_search_result | 文件搜索结果 |
| completions | 补全结果 |
| agent_event/team_event | Agent/团队事件 |
| lsp_event/mcp_event/plugin_event/skill_event/ide_event/agent_settings_event | 子系统事件 |

### 4.2 JSON-RPC WebSocket (`/api/rpc/ws`)

- **路径**：`GET /api/rpc/ws`
- **方向**：双向
- **状态**：**外部可访问**（较新的 API 通道）
- **协议**：JSON-RPC 2.0 框架

**帧格式** (`JsonRpcFrame`)：
```json
// 请求
{"type":"request", "jsonrpc":"2.0", "id": 1, "request": {"method":"SessionList","params":{}}}
// 响应
{"type":"response", "jsonrpc":"2.0", "id": 1, "response": {"method":"SessionList","result":{...}}}
// 错误
{"type":"error", "jsonrpc":"2.0", "id": 1, "error": {"code":"validation","error":"..."}}
// 通知（服务端推送）
{"type":"notification", "jsonrpc":"2.0", "notification": {"type":"...","payload":{...}}}
```

`ClientRequest`/`ClientResponse` 的 method 字段与 `ApiMethod` 枚举对应（如 `SessionList`, `Chat`, `AgentsList` 等）。

### 4.3 Terminal WebSocket (`/api/terminal/sessions/{id}/ws`)

- **路径**：`ANY /api/terminal/sessions/{id}/ws`
- **方向**：双向
- **状态**：**外部可访问**
- **协议**：xterm.js PTY 桥接

**前端 → 后端**：
```json
{"type":"input","data":"ls -la\n"}
{"type":"resize","cols":120,"rows":34}
{"type":"terminate"}
{"type":"detach"}
```

**后端 → 前端**：
```json
{"type":"ready","session_id":"...","profile":"...","pid":...,"status":"Running"}
{"type":"output","data":"终端输出文本..."}
{"type":"exit","code":0}
{"type":"error","message":"..."}
```

### 4.4 TUI WebSocket (`/api/tui/ws`)

- **路径**：`ANY /api/tui/ws`
- **方向**：双向
- **状态**：**向后兼容的 shim**，委派给 Terminal WS

此端点是 `/api/terminal/sessions/{id}/ws` 的旧版 shim，自动创建 allthecodes profile 的终端会话。

---

## 5. SSE 事件流

**路径**：`GET /events?client_id=...&last_event_id=...`

| 属性 | 值 |
|---|---|
| 服务 | allthecodes-daemon |
| 方向 | Server → Client (Unidirectional) |
| 状态 | **内部接口** |
| 协议 | Server-Sent Events (text/event-stream) |

### 5.1 SSE 事件类型

| event 字段 | 数据载荷 key |
|---|---|
| daemon_command | command_id, worker_id, kind |
| daemon_{event_type} | worker_id, command_id, event_type, data, created_at |
| stream_start | message_id, tools, model, session_id |
| stream_delta | message_id, event (StreamEvent), session_id |
| assistant_message | message_id, message, session_id |
| user_replay | message_id, content, session_id |
| stream_end | message_id, subtype, is_error, duration_ms, result, session_id |
| tombstone | message_id, assistant_id, session_id |
| api_retry | message_id, attempt, max_retries, retry_delay_ms, error_status, error, session_id |
| compact_boundary | message_id, compact_metadata, session_id |
| tool_use_summary | message_id, summary, preceding_tool_use_ids, session_id |
| goal_updated | message_id, event, goal, session_id |
| system_info | text, level |
| plan_workflow_event | event, summary, record |

### 5.2 SSE 认证

SSE 端点不需要控制令牌认证。客户端用 `client_id` 标识自己，用 `last_event_id` 实现断线重连保护。

---

## 6. IPC / JSON-Lines 协议

### 6.1 传输层

| 属性 | 值 |
|---|---|
| 位置 | `allthecodes-ipc-transport` crate |
| 传输 | stdin/stdout JSON-Lines（\n 分隔） |
| 方向 | 双向（每行独立） |
| 状态 | **内部接口**（后端与 UI 进程间） |

### 6.2 IPC 帧

```rust
pub struct IpcFrame {
    pub line: String,  // 一个完整的 JSON 行
}
```

### 6.3 IPC v1 协议（FrontendMessage / BackendMessage）

**位置**：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/crates/allthecodes-ipc-protocol/src/protocol/mod.rs`

这是最常被前端使用的核心消息协议，参见[第 4 节](#4-websocket-端点)的消息类型详表。

### 6.4 IPC v2 协议（IpcPayload）

**位置**：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/crates/allthecodes-ipc-protocol/src/payload.rs`

**帧结构** (`IpcEnvelope<T>`)：
```json
{
  "version": 1,
  "id": "event-uuid",
  "seq": 1,
  "timestamp": 1700000000,
  "session_id": "session-1",
  "turn_id": "turn-1",
  "run_id": "run-1",
  "correlation_id": "corr-1",
  "payload": { ... }
}
```

**Payload 类型** (`IpcPayload`)：

| kind | 方向 | 说明 |
|---|---|---|
| hello | C→S | 客户端握手 `{client_name, client_version, supported_versions, capabilities}` |
| ready | S→C | 服务端就绪 `{accepted_version, session_id, model, cwd, ...}` |
| client_request | C→S | JSON-RPC 风格请求 `{request_id, request: ClientRequest}` |
| client_notification | C→S | 客户端通知 `{notification_id?, method: SubmitPrompt|AbortQuery|SlashCommand|Quit, params}` |
| client_response | C→S | 客户端响应 `{request_id, result}` |
| client_error | C→S | 客户端错误 `{request_id, code, message, data?}` |
| server_notification | S→C | 服务端通知（包含对话事件、工具事件等规范化载荷） |
| server_request | S→C | 服务端请求 `{request_id, method: PermissionDecision|AskUserQuestion, params, timeout_ms?}` |
| server_error | S→C | 服务端错误 `{request_id?, code, message, data?}` |
| lagged | S→C | 丢失事件通知 `{skipped, last_dropped_type?}` |

### 6.5 子系统事件与命令

**位置**：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/crates/allthecodes-ipc-protocol/src/subsystem_events.rs`

| 子系统 | 事件 (S→C) | 命令 (C→S) |
|---|---|---|
| LSP | ServerStateChanged, DiagnosticsPublished, DiagnosticsSnapshot, DocumentSynced, CompletionResults, CommandError, ServerList, RecommendationRequest, SettingsSnapshot, RecommendationDecision | StartServer, StopServer, RestartServer, QueryStatus, QueryDiagnostics, OpenDocument, ChangeDocument, SaveDocument, CloseDocument, Completion, QuerySettings, RecommendationResponse, UnmutePlugin, SetRecommendationsDisabled |
| MCP | ServerStateChanged, ToolsDiscovered, ResourcesDiscovered, ChannelNotification, ServerList, ConfigList, ConfigChanged, ConfigError, AuthStarted, AuthStatus | ConnectServer, DisconnectServer, ReconnectServer, QueryStatus, QueryConfig, UpsertConfig, RemoveConfig, ToggleEnabled, StartAuth, CompleteAuth, ClearAuth, QueryAuth |
| Plugin | StatusChanged, PluginList, RefreshNeeded, Reloaded, Installed, Updated, Uninstalled, ValidationFailed, ConfigChanged | Enable, Disable, QueryStatus, Reload, Uninstall |
| Skill | SkillsLoaded, SkillList | Reload, QueryStatus |
| IDE | IdeList, SelectionChanged, ConnectionStateChanged | Detect, Select, Clear, Reconnect, QueryStatus |
| AgentSettings | List, Changed, Error, ToolList, EditorOpened, GenerateStarted, Generated | QueryList, Upsert, Delete, QueryTools, OpenInEditor, Generate |

---

## 7. Gateway 远程控制 API

**位置**：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/crates/allthecodes-gateway/src/`  
**挂载点**：由 `allthecodes-daemon` 的 `gateway_routes.rs` 挂载到 `GET /remote-control/v1/*` 路径  
**状态**：**内部接口**（需要 daemon 控制令牌认证）

### 7.1 网关 HTTP 端点

| 路径 | 方法 | 用途 | 方向 |
|---|---|---|---|
| /remote-control/v1/capabilities | GET | 获取 Gateway 能力列表 | C→S |
| /remote-control/v1/runs | POST | 创建运行请求 | C→S |
| /remote-control/v1/runs/{run_id} | GET | 查询运行状态 | C→S |
| /remote-control/v1/runs/{run_id}/events | GET | 获取运行事件 | C→S |
| /remote-control/v1/runs/{run_id}/stop | POST | 停止运行 | C→S |
| /remote-control/v1/runs/{run_id}/approval | POST | 回复审批（允许/拒绝工具调用） | C→S |
| /remote-control/v1/runs/{run_id}/ask-user | POST | 回复用户提问 | C→S |
| /remote-control/v1/adapters | GET | 列出适配器 | C→S |
| /remote-control/v1/adapters/{provider}/connect | POST | 连接适配器 | C→S |
| /remote-control/v1/adapters/{provider}/test-message | POST | 测试适配器消息 | C→S |

### 7.2 Gateway 命令协议

```rust
pub enum GatewayCommandKind {
    Submit,           // 提交提示
    Abort,            // 中止运行
    PermissionResponse,  // 权限响应
    AskUserResponse,     // 用户问题响应
}

pub struct GatewayCommand {
    pub kind: GatewayCommandKind,
    pub run_id: String,
    pub session_key: String,
    pub payload: Value,
    pub idempotency_key: Option<String>,
}
```

### 7.3 Gateway 认证

认证模式为 `LoopbackDaemonToken`，通过 `x-allthecodes-daemon-token` 头或 `Authorization: Bearer` 头传递。同时校验 `Origin` 头。

### 7.4 支持的适配器

- **飞书 (Lark)** - `adapters/lark.rs`
- **Telegram** - `adapters/telegram.rs`

---

## 8. MCP 协议

### 8.1 MCP 客户端

**位置**：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/crates/allthecodes-mcp/src/`

**支持的传输方式**：
| 传输方式 | 状态 | 说明 |
|---|---|---|
| stdio | 主要 | 子进程 stdin/stdout |
| SSE | 遗留 | HTTP Server-Sent Events |
| Streamable HTTP | 当前 | MCP HTTP transport |

**协议标准**：[Model Context Protocol](https://modelcontextprotocol.io/specification/2025-03-26/)

### 8.2 JSON-RPC 2.0 框架

```json
// 请求
{"jsonrpc":"2.0", "id": 1, "method": "tools/list", "params": {...}}
// 通知（无 ID）
{"jsonrpc":"2.0", "method": "notifications/initialized"}
// 成功响应
{"jsonrpc":"2.0", "id": 1, "result": {"tools": [...]}}
// 错误响应
{"jsonrpc":"2.0", "id": 1, "error": {"code": -32600, "message": "Invalid Request"}}
```

### 8.3 MCP 标准方法

| 方法 | 方向 | 说明 |
|---|---|---|
| initialize | C→S→C | 握手（客户端发送，服务端响应） |
| notifications/initialized | C→S | 通知服务端初始化完成 |
| tools/list | C→S→C | 列出工具 |
| tools/call | C→S→C | 调用工具 |
| resources/list | C→S→C | 列出资源 |
| resources/read | C→S→C | 读取资源 |
| prompts/list | C→S→C | 列出提示（可选） |
| prompts/get | C→S→C | 获取提示（可选） |
| logging/message | S→C | 日志消息通知 |

### 8.4 MCP 子系统事件

`McpSubsystemEvent` 枚举：
- `ServerStateChanged { server_name, state, error }`
- `ToolsDiscovered { server_name, tools }`
- `ResourcesDiscovered { server_name, resources }`
- `ChannelNotification { server_name, content, meta }`

### 8.5 MCP 服务器配置

```json
{
  "name": "server-name",
  "type": "stdio|sse|streamable-http",
  "command": "npx",
  "args": ["-y", "@modelcontextprotocol/server-filesystem"],
  "url": "https://...",
  "headers": {"Authorization": "Bearer ..."},
  "env": {"HOME": "/tmp"},
  "oauth": { /* OAuth 配置 */ },
  "browserMcp": true,
  "disabled": false
}
```

---

## 9. Webhook 入口

| 路径 | 服务 | 认证方式 |
|---|---|---|
| /webhook/github | allthecodes-daemon | HMAC-SHA256 via X-Hub-Signature-256 |
| /webhook/slack | allthecodes-daemon | HMAC-SHA256 via X-Slack-Signature |
| /webhook/generic | allthecodes-daemon | 可选秘密令牌 |
| /remote-control/v1/webhooks/{route_id} | allthecodes-daemon | 每路由环境变量 `ALLTHECODES_WEBHOOK_{ROUTE}_SECRET` |

Webhook 秘密令牌来源：
1. `ALLTHECODES_WEBHOOK_{ROUTE_UC}_SECRET` 环境变量（推荐）
2. 特定 `ALLTHECODES_GITHUB_WEBHOOK_SECRET` / `ALLTHECODES_SLACK_WEBHOOK_SECRET` / `ALLTHECODES_GENERIC_WEBHOOK_SECRET`

---

## 10. 前端 Config/Settings API

### 10.1 Web UI 首选项配置

通过 `/api/web/*` 端点组管理：

| 分组 | 端点 | 用途 |
|---|---|---|
| 首选项 | GET/PUT /api/web/preferences | 全局首选项（作为 JSON 对象存储） |
| 首选项字段 | GET /api/web/preferences/fields?keys=... | 按字段键筛选首选项 |
| 主题方案 | GET/POST /api/web/themes, PUT/DELETE /api/web/themes/{id} | 主题方案 CRUD |
| 提示词 | GET/POST /api/web/prompts, PUT/DELETE /api/web/prompts/{id} | 提示词 CRUD（支持 category/tag/favorite 筛选） |
| 工作区布局 | GET/POST /api/web/layouts, PUT/DELETE /api/web/layouts/{id}, POST .../set-default | 工作区布局 CRUD |

所有 `/api/web/*` 端点支持 `?profile_id=...` 查询参数实现多 profile 配置隔离。

### 10.2 全局设置 API

通过标准 REST 端点：

| 端点 | 方法 | 用途 |
|---|---|---|
| /api/settings | POST | 应用设置（SettingsApply） |
| /api/command | POST | 执行命令（CommandRun） |
| /api/providers | GET/POST/PATCH/DELETE | 模型提供商配置 |
| /api/models | GET/PATCH | 模型配置 |
| /api/profiles | GET/POST/PATCH/DELETE | 用户 profile 管理 |
| /api/memory/config | GET/PATCH | 记忆配置 |
| /api/mcp-servers | GET/POST/PATCH/DELETE | MCP 服务器配置 |
| /api/plugins | GET/POST | 插件配置 |

### 10.3 IPC 子系统通道配置

前端通过 IPC WebSocket 发送子系统命令来配置各项功能：

| 子系统 | 配置命令 |
|---|---|
| MCP | UpsertConfig, RemoveConfig, ToggleEnabled, StartAuth, CompleteAuth |
| LSP | QuerySettings, SetRecommendationsDisabled, UnmutePlugin |
| Plugin | Enable, Disable, Reload, Uninstall |
| AgentSettings | QueryList, Upsert, Delete, QueryTools |
| IDE | Select, Clear |

---

## 11. 认证与授权

### 11.1 Daemon 控制令牌

- **传递方式**：`x-allthecodes-daemon-token` HTTP 头 或 `Authorization: Bearer <token>`
- **作用范围**：allthecodes-daemon 的 `/api/*` 端点和 Gateway 的 `/remote-control/v1/*` 端点
- **状态**：内部接口

### 11.2 Webhook 认证

| 平台 | 算法 | 头字段 |
|---|---|---|
| GitHub | HMAC-SHA256 | X-Hub-Signature-256 |
| Slack | HMAC-SHA256 | X-Slack-Signature |
| 通用 | 预共享秘密令牌 | ALLTHECODES_WEBHOOK_{ROUTE}_SECRET 环境变量 |

### 11.3 OAuth 认证

MCP 服务器支持 OAuth 认证流程（`McpCommand::StartAuth` → `McpCommand::CompleteAuth`），令牌存储在 allthecodes 数据目录下。

### 11.4 Gateway 认证

`GatewayAuthVerifier` trait 支持多种认证模式，daemon 使用 `GatewayAuthMode::LoopbackDaemonToken`，同时校验 Origin 头。

### 11.5 前端认证

前端 `/api/auth/*` 端点：
- `AuthStatus` (GET /api/auth/status) - 检查认证状态
- `AuthLogin` (POST /api/auth/login) - 登录
- `AuthLogout` (POST /api/auth/logout) - 登出
- `AuthRefresh` (POST /api/auth/refresh) - 刷新令牌

---

## 12. 前端依赖的端点汇总

以下表格列出前端（`allthecodes-web` React SPA）实际依赖的 API 端点：

### 12.1 主要 API 通道

| 端点类型 | 路径 | 用途 | 嵌入方式 |
|---|---|---|---|
| HTTP REST | `/api/*` (全部约 130+ 端点) | 数据 CRUD、会话管理、设置 | fetch / axios |
| IPC WebSocket | `/api/ipc/ws` | 实时聊天、流式输出、工具调用 | WebSocket |
| Terminal WebSocket | `/api/terminal/sessions/{id}/ws` | 嵌入式终端 (xterm.js) | WebSocket |
| JSON-RPC WebSocket | `/api/rpc/ws` | 替代性 API 通道 | WebSocket |
| TUI WebSocket | `/api/tui/ws` | 旧版 TUI 兼容 | WebSocket |
| SPA Fallback | `/` (所有未匹配路径) | 前端静态文件 | HTTP |

### 12.2 前端高频调用的 Rest 端点

按使用频率排列：

```
高频:
  GET  /api/state                     - UI 状态（当前模型、会话等）
  GET  /api/sessions                  - 会话列表
  POST /api/chat                      - 提交聊天消息
  GET  /api/healthz                   - 健康检查
  GET  /api/capabilities              - 后端能力发现
  GET  /api/providers                 - 模型提供商列表
  GET  /api/models                    - 模型列表

中频:
  GET  /api/sessions/{id}             - 会话详情
  POST /api/sessions/new              - 创建新会话
  POST /api/sessions/{id}/resume      - 恢复会话
  POST /api/sessions/{id}/archive     - 归档会话
  GET/POST/PATCH/DELETE /api/agents/{name} - Agent 管理
  GET/POST/PATCH/DELETE /api/mcp-servers   - MCP 服务器管理
  GET/POST/PATCH/DELETE /api/files/*       - 文件操作
  GET  /api/settings                  - 设置获取
  POST /api/settings                  - 设置更新
  GET/PUT /api/web/preferences        - 前端首选项

低频:
  GET/POST/PUT/DELETE /api/web/themes/*        - 主题管理
  GET/POST/PUT/DELETE /api/web/prompts/*       - 提示词管理
  GET/POST/PUT/DELETE /api/web/layouts/*       - 布局管理
  POST /api/data/export               - 数据导出
  POST /api/data/import               - 数据导入
  GET  /api/jobs                      - 定时任务管理
  GET  /api/group-chat/rooms          - 群聊管理
```

### 12.3 前端 IPC WebSocket 消息使用

前端通过 IPC WebSocket 主要发送：
- `submit_prompt` - 用户输入提交
- `abort_query` - 中止
- `permission_response` - 工具权限审批
- `slash_command` - 斜杠命令

前端通过 IPC WebSocket 主要接收：
- `stream_start` / `stream_delta` / `stream_end` - AI 流式输出
- `thinking_delta` - 思考过程
- `tool_use` / `tool_result` - 工具调用与结果
- `permission_request` - 权限请求弹窗
- `usage_update` - 用量统计
- `error` - 错误通知

---

## 附录：关键代码文件索引

| 组件 | 文件路径 | 内容 |
|---|---|---|
| 协议定义（REST） | `crates/allthecodes-protocol/src/request.rs` | 所有 REST 端点声明（`api_definitions!` 宏） |
| 协议宏 | `crates/allthecodes-protocol/src/macros.rs` | 协议生成宏 |
| 协议传输 | `crates/allthecodes-protocol/src/transport.rs` | JSON-RPC 帧结构、Transport trait |
| 协议错误 | `crates/allthecodes-protocol/src/error.rs` | ApiError 枚举 |
| Web 路由注册 | `crates/allthecodes-web/src/handler_registry.rs` | ApiMethod → handler 映射 |
| Web 路由构建 | `crates/allthecodes-web/src/mod.rs` | Axum Router 组装 |
| Web state | `crates/allthecodes-web/src/web_state_routes.rs` | `/api/web/*` 前端专用路由 |
| Web API 分发 | `crates/allthecodes-web/src/api_dispatcher.rs` | API 请求分发器 |
| JSON-RPC WS | `crates/allthecodes-web/src/ws/api_rpc.rs` | `/api/rpc/ws` JSON-RPC 端点 |
| IPC WS | `crates/allthecodes-web/src/ws/ipc.rs` | `/api/ipc/ws` IPC 端点 |
| Terminal WS | `crates/allthecodes-web/src/ws/terminal.rs` | `/api/terminal/sessions/{id}/ws` 端点 |
| TUI WS shim | `crates/allthecodes-web/src/ws/tui.rs` | `/api/tui/ws` 旧版兼容 |
| Daemon 路由 | `crates/allthecodes-daemon/src/routes.rs` | 后端 daemon API 路由 |
| Daemon SSE | `crates/allthecodes-daemon/src/sse.rs` | SSE 事件流 |
| Daemon server | `crates/allthecodes-daemon/src/server.rs` | Daemon HTTP 服务启动 |
| Daemon Gateway | `crates/allthecodes-daemon/src/gateway_routes.rs` | Gateway 路由挂载 |
| Daemon Webhook | `crates/allthecodes-daemon/src/webhook.rs` | Webhook 处理 |
| Daemon State | `crates/allthecodes-daemon/src/state.rs` | SSE 事件缓冲、客户端管理 |
| IPC 协议 v1 | `crates/allthecodes-ipc-protocol/src/protocol/mod.rs` | FrontendMessage / BackendMessage |
| IPC 协议 v2 | `crates/allthecodes-ipc-protocol/src/payload.rs` | IpcPayload 枚举 |
| IPC 信封 | `crates/allthecodes-ipc-protocol/src/envelope.rs` | IpcEnvelope 结构 |
| IPC 归一化 | `crates/allthecodes-ipc-protocol/src/normalized.rs` | LegacyBackendPayload |
| IPC 子系统事件 | `crates/allthecodes-ipc-protocol/src/subsystem_events.rs` | LSP/MCP/Plugin/Skill/IDE/Agent 子系统事件 |
| IPC 子系统类型 | `crates/allthecodes-ipc-protocol/src/subsystem_types.rs` | 共享 DTO 类型 |
| IPC 传输 | `crates/allthecodes-ipc-transport/src/` | JSONL stdio 传输 |
| IPC 运行时 | `crates/allthecodes-ipc/src/` | IPC 运行时、子系统处理 |
| Gateway API | `crates/allthecodes-gateway/src/api.rs` | 远程控制 HTTP API |
| Gateway Runner | `crates/allthecodes-gateway/src/runner.rs` | 运行管理、GatewayCommand |
| Gateway Run | `crates/allthecodes-gateway/src/run.rs` | RunRequest/RunMeta/RunStatus |
| MCP 协议 | `crates/allthecodes-mcp/src/lib.rs` | MCP 类型定义、JSON-RPC 框架 |
| MCP 客户端 | `crates/allthecodes-mcp/src/client/` | stdio/SSE/HTTP 传输客户端 |

---

*本文档由 API 分析 agent 自动生成。后续端点变更时应对应更新。*
