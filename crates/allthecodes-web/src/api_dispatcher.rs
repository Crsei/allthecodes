//! Shared API dispatcher for transport adapters.
//!
//! REST handlers, direct in-process calls, and future API WebSocket adapters
//! should converge here once their DTOs are stable. PTY, MCP, browser native
//! host, and daemon SSE remain outside this dispatcher boundary.

use async_trait::async_trait;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use tracing::Instrument;

use allthecodes_protocol::v1;
use allthecodes_protocol::{
    ApiError, ApiMethod, ClientRequest, ClientResponse, MessageProcessor, NoParams,
    ServerNotification,
};

use crate::handlers;
use crate::processors::{dispatch_processor, protocol_error_response, Processor};
use crate::state::WebState;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApiConnectionId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApiTransportKind {
    Rest,
    JsonRpcWebSocket,
    Direct,
    IpcBridge,
    DedicatedWebSocket,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiRequestContext {
    pub connection_id: Option<ApiConnectionId>,
    pub transport: ApiTransportKind,
}

impl ApiRequestContext {
    pub fn rest() -> Self {
        Self {
            connection_id: None,
            transport: ApiTransportKind::Rest,
        }
    }

    pub fn direct() -> Self {
        Self {
            connection_id: None,
            transport: ApiTransportKind::Direct,
        }
    }

    pub fn json_rpc_websocket(connection_id: ApiConnectionId) -> Self {
        Self {
            connection_id: Some(connection_id),
            transport: ApiTransportKind::JsonRpcWebSocket,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApiDispatcherMigrationState {
    Dispatched,
    LegacyRestHandler,
    DedicatedTransport,
    LegacyIpcBridge,
}

// ApiDispatcher migration tracker for every ClientRequest variant declared in
// allthecodes_protocol::request. Keep this list in sync when adding new
// protocol operations so JSON-RPC rollout gaps stay visible.
//
// ClientRequest::Health - dispatched.
// ClientRequest::Chat - SSE chat transport, excluded from JSON dispatcher.
// ClientRequest::Abort - legacy REST handler, ChatProcessor target.
// ClientRequest::State - legacy REST handler, ChatProcessor target.
// ClientRequest::SystemPrompt - legacy REST handler, ChatProcessor target.
// ClientRequest::CodingAgentsStatus - legacy REST handler, ChatProcessor target.
// ClientRequest::LaunchpadSnapshotCreate - legacy REST handler.
// ClientRequest::SessionList - dispatched.
// ClientRequest::SessionSearch - dispatched.
// ClientRequest::SessionCreate - dispatched.
// ClientRequest::SessionDetail - dispatched.
// ClientRequest::SessionResume - dispatched.
// ClientRequest::SessionArchive - dispatched.
// ClientRequest::SessionModePatch - dispatched.
// ClientRequest::SessionMessageBranch - dispatched.
// ClientRequest::SessionMessageFeedback - dispatched.
// ClientRequest::SessionMessageDelete - dispatched.
// ClientRequest::SessionMessageRegeneratePrepare - dispatched.
// ClientRequest::SessionMessageEditPrepare - dispatched.
// ClientRequest::SessionMessageRollbackPreview - dispatched.
// ClientRequest::SessionMessageRollback - dispatched.
// ClientRequest::Capabilities - dispatched.
// ClientRequest::ChatModesList - dispatched.
// ClientRequest::ChatModesResources - dispatched.
// ClientRequest::ChatModesUpsert - legacy REST handler, ChatModesProcessor target.
// ClientRequest::ChatModesDelete - legacy REST handler, ChatModesProcessor target.
// ClientRequest::AgentsList - legacy REST handler, AgentProcessor target.
// ClientRequest::AgentsCreate - legacy REST handler, AgentProcessor target.
// ClientRequest::AgentsDetail - legacy REST handler, AgentProcessor target.
// ClientRequest::AgentsUpdate - legacy REST handler, AgentProcessor target.
// ClientRequest::AgentsDelete - legacy REST handler, AgentProcessor target.
// ClientRequest::AgentsRestore - legacy REST handler, AgentProcessor target.
// ClientRequest::PeopleList - dispatched.
// ClientRequest::PeopleCreate - dispatched.
// ClientRequest::PeopleDetail - legacy REST handler, PeopleProcessor target.
// ClientRequest::PeopleUpdate - legacy REST handler, PeopleProcessor target.
// ClientRequest::PeopleDelete - legacy REST handler, PeopleProcessor target.
// ClientRequest::HooksList - legacy REST handler, HooksProcessor target.
// ClientRequest::HooksCreate - legacy REST handler, HooksProcessor target.
// ClientRequest::HooksTest - legacy REST handler, HooksProcessor target.
// ClientRequest::HooksDetail - legacy REST handler, HooksProcessor target.
// ClientRequest::HooksUpdate - legacy REST handler, HooksProcessor target.
// ClientRequest::HooksDelete - legacy REST handler, HooksProcessor target.
// ClientRequest::PromptsList - dispatched.
// ClientRequest::PromptsCreate - dispatched.
// ClientRequest::PromptsDetail - legacy REST handler, PromptsProcessor target.
// ClientRequest::PromptsUpdate - legacy REST handler, PromptsProcessor target.
// ClientRequest::PromptsDelete - legacy REST handler, PromptsProcessor target.
// ClientRequest::McpServersList - legacy REST handler, McpProcessor target.
// ClientRequest::McpServersHealth - legacy REST handler, McpProcessor target.
// ClientRequest::McpServersProbe - legacy REST handler, McpProcessor target.
// ClientRequest::McpServersCreate - legacy REST handler, McpProcessor target.
// ClientRequest::McpServersMarketplace - legacy REST handler, McpProcessor target.
// ClientRequest::McpServersDetail - legacy REST handler, McpProcessor target.
// ClientRequest::McpServersUpdate - legacy REST handler, McpProcessor target.
// ClientRequest::McpServersDelete - legacy REST handler, McpProcessor target.
// ClientRequest::PluginsList - dispatched.
// ClientRequest::PluginsInstalled - dispatched.
// ClientRequest::PluginsMarketplace - dispatched.
// ClientRequest::PluginsInstall - dispatched.
// ClientRequest::PluginsUpdate - dispatched.
// ClientRequest::PluginsUninstallById - dispatched.
// ClientRequest::PluginsEnable - dispatched.
// ClientRequest::PluginsDisable - dispatched.
// ClientRequest::PluginsRestart - dispatched.
// ClientRequest::PluginsTestConnection - legacy REST path-parameter handler.
// ClientRequest::PluginsUninstall - legacy REST path-parameter handler.
// ClientRequest::ChannelsList - legacy REST handler, ChannelsProcessor target.
// ClientRequest::ChannelsCapabilities - legacy REST handler, ChannelsProcessor target.
// ClientRequest::ChannelsConnect - legacy REST handler, ChannelsProcessor target.
// ClientRequest::ChannelsTest - legacy REST handler, ChannelsProcessor target.
// ClientRequest::GatewayStatus - dispatched.
// ClientRequest::GatewaysList - dispatched.
// ClientRequest::GatewayStart - legacy REST handler, GatewayProcessor target.
// ClientRequest::GatewayStop - legacy REST handler, GatewayProcessor target.
// ClientRequest::ComputerUseStatus - legacy REST handler, ComputerUseProcessor target.
// ClientRequest::ComputerUsePermissionRequest - legacy REST handler, ComputerUseProcessor target.
// ClientRequest::ComputerUseTest - legacy REST handler, ComputerUseProcessor target.
// ClientRequest::AppshotsStatus - legacy REST handler, AppshotsProcessor target.
// ClientRequest::AppshotsCapture - legacy REST handler, AppshotsProcessor target.
// ClientRequest::ChromeRelayStatus - legacy REST handler, ChromeRelayProcessor target.
// ClientRequest::ChromeRelayLaunch - legacy REST handler, ChromeRelayProcessor target.
// ClientRequest::ChromeRelayTokenRegenerate - legacy REST handler, ChromeRelayProcessor target.
// ClientRequest::ActivityRecorderStatus - legacy REST handler, ActivityRecorderProcessor target.
// ClientRequest::ActivityRecorderSessions - legacy REST handler, ActivityRecorderProcessor target.
// ClientRequest::ActivityRecorderClear - legacy REST handler, ActivityRecorderProcessor target.
// ClientRequest::SettingsApply - legacy REST handler, AdminProcessor target.
// ClientRequest::CommandRun - legacy REST handler, AdminProcessor target.
// ClientRequest::MemoryConfigGet - legacy REST handler, MemoryProcessor target.
// ClientRequest::MemoryConfigPatch - legacy REST handler, MemoryProcessor target.
// ClientRequest::MemoryList - legacy REST handler, MemoryProcessor target.
// ClientRequest::MemoryUpdate - legacy REST handler, MemoryProcessor target.
// ClientRequest::SpeechModels - legacy REST handler, SpeechProcessor target.
// ClientRequest::SpeechModelDownload - legacy REST handler, SpeechProcessor target.
// ClientRequest::SpeechModelDelete - legacy REST handler, SpeechProcessor target.
// ClientRequest::SearchCookiesExport - legacy REST handler, SearchProcessor target.
// ClientRequest::SearchCookiesImport - legacy REST handler, SearchProcessor target.
// ClientRequest::SearchCookiesClear - legacy REST handler, SearchProcessor target.
// ClientRequest::DataExport - legacy REST handler, DataProcessor target.
// ClientRequest::DataImport - legacy REST handler, DataProcessor target.
// ClientRequest::TokenSavings - legacy REST handler, UsageProcessor target.
// ClientRequest::DebugState - legacy REST handler, DebugProcessor target.
// ClientRequest::DebugSessionTrace - legacy REST handler, DebugProcessor target.
// ClientRequest::DebugAction - legacy REST handler, DebugProcessor target.
// ClientRequest::ProtocolRoutes - legacy REST handler, registry diagnostics target.
// ClientRequest::WorkspacesList - legacy REST handler, WorkspaceProcessor target.
// ClientRequest::WorkspacePatch - legacy REST handler, WorkspaceProcessor target.
// ClientRequest::WorkspaceOpen - legacy REST handler, WorkspaceProcessor target.
// ClientRequest::WorkspaceSessionsArchive - legacy REST handler, WorkspaceProcessor target.
// ClientRequest::AuthStatus - legacy REST handler, AuthProcessor target.
// ClientRequest::AuthLogin - legacy REST handler, AuthProcessor target.
// ClientRequest::AuthLogout - legacy REST handler, AuthProcessor target.
// ClientRequest::AuthRefresh - legacy REST handler, AuthProcessor target.
// ClientRequest::ProfilesList - legacy REST handler, ProfileProcessor target.
// ClientRequest::ProfilesCreate - legacy REST handler, ProfileProcessor target.
// ClientRequest::ProfilesImport - legacy REST handler, ProfileProcessor target.
// ClientRequest::ProfilesDetail - legacy REST handler, ProfileProcessor target.
// ClientRequest::ProfilesUpdate - legacy REST handler, ProfileProcessor target.
// ClientRequest::ProfilesDelete - legacy REST handler, ProfileProcessor target.
// ClientRequest::ProfilesSwitch - legacy REST handler, ProfileProcessor target.
// ClientRequest::ProfilesExport - binary export transport, excluded from JSON dispatcher.
// ClientRequest::ProvidersList - legacy REST handler, ProviderProcessor target.
// ClientRequest::ProvidersCreate - legacy REST handler, ProviderProcessor target.
// ClientRequest::ProvidersUpdate - legacy REST handler, ProviderProcessor target.
// ClientRequest::ProvidersDelete - legacy REST handler, ProviderProcessor target.
// ClientRequest::ProvidersRefreshModels - legacy REST handler, ProviderProcessor target.
// ClientRequest::ModelsList - dispatched.
// ClientRequest::ModelsUpdate - legacy REST handler, ModelProcessor target.
// ClientRequest::ModelsSetDefault - legacy REST handler, ModelProcessor target.
// ClientRequest::Credentials - legacy REST handler, AuthProcessor target.
// ClientRequest::OAuthStart - legacy REST handler, AuthProcessor target.
// ClientRequest::OAuthPoll - legacy REST handler, AuthProcessor target.
// ClientRequest::Logs - legacy REST handler, DiagnosticsProcessor target.
// ClientRequest::LogsExport - binary export transport, excluded from JSON dispatcher.
// ClientRequest::DiagnosticsSnapshot - legacy REST handler, DiagnosticsProcessor target.
// ClientRequest::DiagnosticsTraces - legacy REST handler, DiagnosticsProcessor target.
// ClientRequest::TerminalProfiles - dedicated terminal transport, excluded.
// ClientRequest::TerminalSessionsList - dedicated terminal transport, excluded.
// ClientRequest::TerminalSessionsCreate - dedicated terminal transport, excluded.
// ClientRequest::TerminalSessionDetail - dedicated terminal transport, excluded.
// ClientRequest::TerminalSessionOutput - dedicated terminal output replay, excluded.
// ClientRequest::TerminalSessionDelete - dedicated terminal transport, excluded.
// ClientRequest::TerminalSessionWs - dedicated terminal WebSocket, excluded.
// ClientRequest::TuiWs - dedicated TUI WebSocket, excluded.
// ClientRequest::IpcWs - legacy IPC bridge, excluded.
// ClientRequest::GitLog - legacy REST handler, GitProcessor target.
// ClientRequest::GitDiff - legacy REST handler, GitProcessor target.
// ClientRequest::WorktreeSessionsList - dispatched.
// ClientRequest::WorktreeSessionsCurrent - dispatched.
// ClientRequest::WorktreeSessionsBySession - dispatched.
// ClientRequest::Proxy - legacy REST handler, ProxyProcessor target.
// ClientRequest::Usage - legacy REST handler, UsageProcessor target.
// ClientRequest::FilesTree - dispatched.
// ClientRequest::FilesStat - dispatched.
// ClientRequest::FilesRead - dispatched.
// ClientRequest::FilesWrite - dispatched.
// ClientRequest::FilesUpload - dispatched.
// ClientRequest::FilesDownload - binary download transport, excluded from JSON dispatcher.
// ClientRequest::FilesMkdir - dispatched.
// ClientRequest::FilesRename - dispatched.
// ClientRequest::FilesCopy - dispatched.
// ClientRequest::FilesMove - dispatched.
// ClientRequest::FilesDelete - dispatched.
// ClientRequest::SkillsList - dispatched.
// ClientRequest::SkillsDetail - legacy REST handler, SkillProcessor target.
// ClientRequest::SkillsPatch - legacy REST handler, SkillProcessor target.
// ClientRequest::SkillsFiles - legacy REST handler, SkillProcessor target.
// ClientRequest::KanbanBoards - dispatched.
// ClientRequest::KanbanBoardDetail - legacy REST handler, KanbanProcessor target.
// ClientRequest::KanbanTaskCreate - dispatched.
// ClientRequest::KanbanTaskUpdate - legacy REST handler, KanbanProcessor target.
// ClientRequest::KanbanTaskComment - legacy REST handler, KanbanProcessor target.
// ClientRequest::JobsList - legacy REST handler, JobsProcessor target.
// ClientRequest::JobsCreate - legacy REST handler, JobsProcessor target.
// ClientRequest::JobsUpdate - legacy REST handler, JobsProcessor target.
// ClientRequest::JobsDelete - legacy REST handler, JobsProcessor target.
// ClientRequest::JobsPause - legacy REST handler, JobsProcessor target.
// ClientRequest::JobsResume - legacy REST handler, JobsProcessor target.
// ClientRequest::JobsRun - legacy REST handler, JobsProcessor target.
// ClientRequest::CronHistory - legacy REST handler, JobsProcessor target.
// ClientRequest::GroupChatRoomsList - legacy REST handler, GroupChatProcessor target.
// ClientRequest::GroupChatRoomCreate - legacy REST handler, GroupChatProcessor target.
// ClientRequest::GroupChatRoomDetail - legacy REST handler, GroupChatProcessor target.
// ClientRequest::GroupChatRoomDelete - legacy REST handler, GroupChatProcessor target.
// ClientRequest::GroupChatRoomClone - legacy REST handler, GroupChatProcessor target.
// ClientRequest::GroupChatInvite - legacy REST handler, GroupChatProcessor target.
// ClientRequest::GroupChatAgentAdd - legacy REST handler, GroupChatProcessor target.
// ClientRequest::GroupChatAgentUpdate - legacy REST handler, GroupChatProcessor target.
// ClientRequest::GroupChatAgentDelete - legacy REST handler, GroupChatProcessor target.
// ClientRequest::GroupChatMessage - legacy REST handler, GroupChatProcessor target.
// ClientRequest::GroupChatCompression - legacy REST handler, GroupChatProcessor target.
// ClientRequest::GroupChatStream - SSE group-chat transport, excluded from JSON dispatcher.
// ClientRequest::BackendServices - legacy REST handler, BackendServicesProcessor target.
// ClientRequest::BackendServicesSessionsSync - legacy REST handler, BackendServicesProcessor target.
// ClientRequest::BackendServicesContextCompressionRun - legacy REST handler, BackendServicesProcessor target.
// ClientRequest::BackendServicesAgentBridgeRetry - legacy REST handler, BackendServicesProcessor target.
// ClientRequest::BackendServicesMigrationsRun - legacy REST handler, BackendServicesProcessor target.
// ClientRequest::BackendServicesBackups - legacy REST handler, BackendServicesProcessor target.
pub(crate) fn dispatcher_migration_state(operation: ApiMethod) -> ApiDispatcherMigrationState {
    crate::api_operation_registry::api_operation_registry().migration_state(operation)
}

#[derive(Clone)]
pub struct ApiDispatcher {
    state: WebState,
    default_context: ApiRequestContext,
}

impl ApiDispatcher {
    pub fn new(state: WebState) -> Self {
        Self {
            state,
            default_context: ApiRequestContext::direct(),
        }
    }

    pub fn with_default_context(mut self, context: ApiRequestContext) -> Self {
        self.default_context = context;
        self
    }

    pub async fn dispatch(
        &self,
        context: ApiRequestContext,
        request: ClientRequest,
    ) -> Result<ClientResponse, ApiError> {
        dispatch(self.state.clone(), context, request).await
    }
}

#[async_trait]
impl MessageProcessor for ApiDispatcher {
    async fn process_request(&self, request: ClientRequest) -> Result<ClientResponse, ApiError> {
        self.dispatch(self.default_context.clone(), request).await
    }

    async fn process_notification(&self, notification: ServerNotification) -> Result<(), ApiError> {
        match notification {}
    }
}

pub async fn dispatch(
    state: WebState,
    context: ApiRequestContext,
    request: ClientRequest,
) -> Result<ClientResponse, ApiError> {
    match request {
        ClientRequest::Health(NoParams {}) => {
            let response = dispatch_tracked_processor::<handlers::HealthProcessor>(
                state,
                context,
                ApiMethod::Health,
                NoParams {},
            )
            .await?;
            Ok(ClientResponse::Health(response))
        }
        ClientRequest::Capabilities(params) => {
            let response = dispatch_tracked_processor::<handlers::CapabilitiesProcessor>(
                state,
                context,
                ApiMethod::Capabilities,
                params,
            )
            .await?;
            Ok(ClientResponse::Capabilities(response))
        }
        ClientRequest::SessionList(NoParams {}) => {
            let response = dispatch_tracked_processor::<handlers::SessionListProcessor>(
                state,
                context,
                ApiMethod::SessionList,
                NoParams {},
            )
            .await?;
            Ok(ClientResponse::SessionList(map_session_list(response)))
        }
        ClientRequest::SessionSearch(params) => {
            let response = dispatch_tracked_processor::<handlers::SessionSearchProcessor>(
                state,
                context,
                ApiMethod::SessionSearch,
                params,
            )
            .await?;
            Ok(ClientResponse::SessionSearch(response))
        }
        ClientRequest::SessionCreate(params) => {
            let response = dispatch_tracked_processor::<handlers::SessionCreateProcessor>(
                state,
                context,
                ApiMethod::SessionCreate,
                params,
            )
            .await?;
            Ok(ClientResponse::SessionCreate(v1::SessionCreateResponse {
                id: response.session_id,
            }))
        }
        ClientRequest::SessionDetail(params) => {
            let response = dispatch_tracked_processor::<handlers::SessionDetailProcessor>(
                state,
                context,
                ApiMethod::SessionDetail,
                params,
            )
            .await?;
            Ok(ClientResponse::SessionDetail(map_session_detail(response)))
        }
        ClientRequest::SessionResume(params) => {
            let response = dispatch_tracked_processor::<handlers::SessionResumeProcessor>(
                state,
                context,
                ApiMethod::SessionResume,
                params,
            )
            .await?;
            Ok(ClientResponse::SessionResume(v1::SessionResumeResponse {
                id: response.session_id,
                resumed: true,
            }))
        }
        ClientRequest::SessionArchive(params) => {
            let id = params.id.clone();
            dispatch_tracked_processor::<handlers::SessionArchiveProcessor>(
                state,
                context,
                ApiMethod::SessionArchive,
                params,
            )
            .await?;
            Ok(ClientResponse::SessionArchive(v1::SessionArchiveResponse {
                id,
                archived: true,
            }))
        }
        ClientRequest::SessionModePatch(params) => {
            let response = dispatch_tracked_processor::<handlers::SessionModePatchProcessor>(
                state,
                context,
                ApiMethod::SessionModePatch,
                params,
            )
            .await?;
            Ok(ClientResponse::SessionModePatch(map_session_mode(response)))
        }
        ClientRequest::SessionMessageBranch(params) => {
            let response = dispatch_tracked_processor::<handlers::SessionMessageBranchProcessor>(
                state,
                context,
                ApiMethod::SessionMessageBranch,
                params,
            )
            .await?;
            Ok(ClientResponse::SessionMessageBranch(response_to_value(
                response,
            )?))
        }
        ClientRequest::SessionMessageFeedback(params) => {
            let response = dispatch_tracked_processor::<handlers::SessionMessageFeedbackProcessor>(
                state,
                context,
                ApiMethod::SessionMessageFeedback,
                params,
            )
            .await?;
            Ok(ClientResponse::SessionMessageFeedback(response_to_value(
                response,
            )?))
        }
        ClientRequest::SessionMessageDelete(params) => {
            let response = dispatch_tracked_processor::<handlers::SessionMessageDeleteProcessor>(
                state,
                context,
                ApiMethod::SessionMessageDelete,
                params,
            )
            .await?;
            Ok(ClientResponse::SessionMessageDelete(response_to_value(
                response,
            )?))
        }
        ClientRequest::SessionMessageRegeneratePrepare(params) => {
            let response =
                dispatch_tracked_processor::<handlers::SessionMessageRegeneratePrepareProcessor>(
                    state,
                    context,
                    ApiMethod::SessionMessageRegeneratePrepare,
                    params,
                )
                .await?;
            Ok(ClientResponse::SessionMessageRegeneratePrepare(
                response_to_value(response)?,
            ))
        }
        ClientRequest::SessionMessageEditPrepare(params) => {
            let response =
                dispatch_tracked_processor::<handlers::SessionMessageEditPrepareProcessor>(
                    state,
                    context,
                    ApiMethod::SessionMessageEditPrepare,
                    params,
                )
                .await?;
            Ok(ClientResponse::SessionMessageEditPrepare(
                response_to_value(response)?,
            ))
        }
        ClientRequest::SessionMessageRollbackPreview(params) => {
            let response =
                dispatch_tracked_processor::<handlers::SessionMessageRollbackPreviewProcessor>(
                    state,
                    context,
                    ApiMethod::SessionMessageRollbackPreview,
                    params,
                )
                .await?;
            Ok(ClientResponse::SessionMessageRollbackPreview(
                response_to_value(response)?,
            ))
        }
        ClientRequest::SessionMessageRollback(params) => {
            let response = dispatch_tracked_processor::<handlers::SessionMessageRollbackProcessor>(
                state,
                context,
                ApiMethod::SessionMessageRollback,
                params,
            )
            .await?;
            Ok(ClientResponse::SessionMessageRollback(response))
        }
        ClientRequest::FilesTree(params) => {
            let response = dispatch_tracked_processor::<handlers::FilesTreeProcessor>(
                state,
                context,
                ApiMethod::FilesTree,
                params,
            )
            .await?;
            Ok(ClientResponse::FilesTree(response))
        }
        ClientRequest::FilesStat(params) => {
            let response = dispatch_tracked_processor::<handlers::FilesStatProcessor>(
                state,
                context,
                ApiMethod::FilesStat,
                params,
            )
            .await?;
            Ok(ClientResponse::FilesStat(response))
        }
        ClientRequest::FilesRead(params) => {
            let response = dispatch_tracked_processor::<handlers::FilesReadProcessor>(
                state,
                context,
                ApiMethod::FilesRead,
                params,
            )
            .await?;
            Ok(ClientResponse::FilesRead(response))
        }
        ClientRequest::FilesPreview(params) => {
            let response = dispatch_tracked_processor::<handlers::FilesPreviewProcessor>(
                state,
                context,
                ApiMethod::FilesPreview,
                params,
            )
            .await?;
            Ok(ClientResponse::FilesPreview(response))
        }
        ClientRequest::FilesWrite(params) => {
            let response = dispatch_tracked_processor::<handlers::FilesWriteProcessor>(
                state,
                context,
                ApiMethod::FilesWrite,
                params,
            )
            .await?;
            Ok(ClientResponse::FilesWrite(response))
        }
        ClientRequest::FilesUpload(params) => {
            let response = dispatch_tracked_processor::<handlers::FilesUploadProcessor>(
                state,
                context,
                ApiMethod::FilesUpload,
                params,
            )
            .await?;
            Ok(ClientResponse::FilesUpload(response))
        }
        ClientRequest::FilesMkdir(params) => {
            let response = dispatch_tracked_processor::<handlers::FilesMkdirProcessor>(
                state,
                context,
                ApiMethod::FilesMkdir,
                params,
            )
            .await?;
            Ok(ClientResponse::FilesMkdir(response))
        }
        ClientRequest::FilesRename(params) => {
            let response = dispatch_tracked_processor::<handlers::FilesRenameProcessor>(
                state,
                context,
                ApiMethod::FilesRename,
                params,
            )
            .await?;
            Ok(ClientResponse::FilesRename(response))
        }
        ClientRequest::FilesCopy(params) => {
            let response = dispatch_tracked_processor::<handlers::FilesCopyProcessor>(
                state,
                context,
                ApiMethod::FilesCopy,
                params,
            )
            .await?;
            Ok(ClientResponse::FilesCopy(response))
        }
        ClientRequest::FilesMove(params) => {
            let response = dispatch_tracked_processor::<handlers::FilesMoveProcessor>(
                state,
                context,
                ApiMethod::FilesMove,
                params,
            )
            .await?;
            Ok(ClientResponse::FilesMove(response))
        }
        ClientRequest::FilesDelete(params) => {
            let response = dispatch_tracked_processor::<handlers::FilesDeleteProcessor>(
                state,
                context,
                ApiMethod::FilesDelete,
                params,
            )
            .await?;
            Ok(ClientResponse::FilesDelete(response))
        }
        ClientRequest::SkillsList(params) => {
            let response = dispatch_tracked_processor::<handlers::SkillsListProcessor>(
                state,
                context,
                ApiMethod::SkillsList,
                params,
            )
            .await?;
            Ok(ClientResponse::SkillsList(response))
        }
        ClientRequest::ChatModesList(NoParams {}) => {
            let response = dispatch_tracked_processor::<handlers::ChatModesListProcessor>(
                state,
                context,
                ApiMethod::ChatModesList,
                NoParams {},
            )
            .await?;
            Ok(ClientResponse::ChatModesList(response))
        }
        ClientRequest::ChatModesResources(NoParams {}) => {
            let response = dispatch_tracked_processor::<handlers::ChatModesResourcesProcessor>(
                state,
                context,
                ApiMethod::ChatModesResources,
                NoParams {},
            )
            .await?;
            Ok(ClientResponse::ChatModesResources(response))
        }
        ClientRequest::PeopleList(NoParams {}) => {
            let response = dispatch_tracked_processor::<handlers::PeopleListProcessor>(
                state,
                context,
                ApiMethod::PeopleList,
                NoParams {},
            )
            .await?;
            Ok(ClientResponse::PeopleList(response))
        }
        ClientRequest::PeopleCreate(params) => {
            let response = dispatch_tracked_processor::<handlers::PeopleCreateProcessor>(
                state,
                context,
                ApiMethod::PeopleCreate,
                params,
            )
            .await?;
            Ok(ClientResponse::PeopleCreate(response))
        }
        ClientRequest::PromptsList(NoParams {}) => {
            let response = dispatch_tracked_processor::<handlers::PromptsListProcessor>(
                state,
                context,
                ApiMethod::PromptsList,
                NoParams {},
            )
            .await?;
            Ok(ClientResponse::PromptsList(response))
        }
        ClientRequest::PromptsCreate(params) => {
            let response = dispatch_tracked_processor::<handlers::PromptsCreateProcessor>(
                state,
                context,
                ApiMethod::PromptsCreate,
                params,
            )
            .await?;
            Ok(ClientResponse::PromptsCreate(response))
        }
        ClientRequest::KanbanBoards(params) => {
            let response = dispatch_tracked_processor::<handlers::KanbanBoardsProcessor>(
                state,
                context,
                ApiMethod::KanbanBoards,
                params,
            )
            .await?;
            Ok(ClientResponse::KanbanBoards(response))
        }
        ClientRequest::KanbanTaskCreate(params) => {
            let response = dispatch_tracked_processor::<handlers::KanbanTaskCreateProcessor>(
                state,
                context,
                ApiMethod::KanbanTaskCreate,
                params,
            )
            .await?;
            Ok(ClientResponse::KanbanTaskCreate(response))
        }
        ClientRequest::PluginsList(NoParams {}) => {
            let response = dispatch_tracked_processor::<handlers::PluginsListProcessor>(
                state,
                context,
                ApiMethod::PluginsList,
                NoParams {},
            )
            .await?;
            Ok(ClientResponse::PluginsList(response))
        }
        ClientRequest::PluginsInstalled(NoParams {}) => {
            let response = dispatch_tracked_processor::<handlers::PluginsListProcessor>(
                state,
                context,
                ApiMethod::PluginsInstalled,
                NoParams {},
            )
            .await?;
            Ok(ClientResponse::PluginsInstalled(response))
        }
        ClientRequest::PluginsMarketplace(NoParams {}) => {
            let response = dispatch_tracked_processor::<handlers::PluginsMarketplaceProcessor>(
                state,
                context,
                ApiMethod::PluginsMarketplace,
                NoParams {},
            )
            .await?;
            Ok(ClientResponse::PluginsMarketplace(response))
        }
        ClientRequest::PluginsInstall(params) => {
            let response = dispatch_tracked_processor::<handlers::PluginsInstallProcessor>(
                state,
                context,
                ApiMethod::PluginsInstall,
                params,
            )
            .await?;
            Ok(ClientResponse::PluginsInstall(response))
        }
        ClientRequest::PluginsUpdate(params) => {
            let response = dispatch_tracked_processor::<handlers::PluginsUpdateProcessor>(
                state,
                context,
                ApiMethod::PluginsUpdate,
                params,
            )
            .await?;
            Ok(ClientResponse::PluginsUpdate(response))
        }
        ClientRequest::PluginsUninstallById(params) => {
            let response = dispatch_tracked_processor::<handlers::PluginsUninstallByIdProcessor>(
                state,
                context,
                ApiMethod::PluginsUninstallById,
                params,
            )
            .await?;
            Ok(ClientResponse::PluginsUninstallById(response))
        }
        ClientRequest::PluginsEnable(params) => {
            let response = dispatch_tracked_processor::<handlers::PluginsEnableProcessor>(
                state,
                context,
                ApiMethod::PluginsEnable,
                params,
            )
            .await?;
            Ok(ClientResponse::PluginsEnable(response))
        }
        ClientRequest::PluginsDisable(params) => {
            let response = dispatch_tracked_processor::<handlers::PluginsDisableProcessor>(
                state,
                context,
                ApiMethod::PluginsDisable,
                params,
            )
            .await?;
            Ok(ClientResponse::PluginsDisable(response))
        }
        ClientRequest::PluginsRestart(params) => {
            let response = dispatch_tracked_processor::<handlers::PluginsRestartProcessor>(
                state,
                context,
                ApiMethod::PluginsRestart,
                params,
            )
            .await?;
            Ok(ClientResponse::PluginsRestart(response))
        }
        ClientRequest::GatewayStatus(NoParams {}) => {
            let response = dispatch_tracked_processor::<handlers::GatewayStatusProcessor>(
                state,
                context,
                ApiMethod::GatewayStatus,
                NoParams {},
            )
            .await?;
            Ok(ClientResponse::GatewayStatus(response))
        }
        ClientRequest::GatewaysList(NoParams {}) => {
            let response = dispatch_tracked_processor::<handlers::GatewaysListProcessor>(
                state,
                context,
                ApiMethod::GatewaysList,
                NoParams {},
            )
            .await?;
            Ok(ClientResponse::GatewaysList(response))
        }
        ClientRequest::ModelsList(NoParams {}) => {
            let response = dispatch_tracked_processor::<handlers::ModelsListProcessor>(
                state,
                context,
                ApiMethod::ModelsList,
                NoParams {},
            )
            .await?;
            Ok(ClientResponse::ModelsList(response))
        }
        ClientRequest::WorktreeSessionsList(params) => {
            let response = dispatch_tracked_processor::<handlers::WorktreeSessionsListProcessor>(
                state,
                context,
                ApiMethod::WorktreeSessionsList,
                params,
            )
            .await?;
            Ok(ClientResponse::WorktreeSessionsList(response))
        }
        ClientRequest::WorktreeSessionsCurrent(NoParams {}) => {
            let response =
                dispatch_tracked_processor::<handlers::WorktreeSessionsCurrentProcessor>(
                    state,
                    context,
                    ApiMethod::WorktreeSessionsCurrent,
                    NoParams {},
                )
                .await?;
            Ok(ClientResponse::WorktreeSessionsCurrent(response))
        }
        ClientRequest::WorktreeSessionsBySession(params) => {
            let response =
                dispatch_tracked_processor::<handlers::WorktreeSessionsBySessionProcessor>(
                    state,
                    context,
                    ApiMethod::WorktreeSessionsBySession,
                    params,
                )
                .await?;
            Ok(ClientResponse::WorktreeSessionsBySession(response))
        }
        other => Err(ApiError::NotImplemented {
            capability: format!("{:?}", other.method()),
        }),
    }
}

pub(crate) async fn rest_processor_response<P>(
    state: WebState,
    operation: ApiMethod,
    params: P::Request,
) -> Response
where
    P: Processor + From<WebState>,
    P::Response: Serialize,
{
    match dispatch_rest_processor::<P>(state, operation, params).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => protocol_error_response(error),
    }
}

pub(crate) async fn dispatch_rest_processor<P>(
    state: WebState,
    operation: ApiMethod,
    params: P::Request,
) -> Result<P::Response, ApiError>
where
    P: Processor + From<WebState>,
{
    dispatch_tracked_processor::<P>(state, ApiRequestContext::rest(), operation, params).await
}

async fn dispatch_tracked_processor<P>(
    state: WebState,
    context: ApiRequestContext,
    operation: ApiMethod,
    params: P::Request,
) -> Result<P::Response, ApiError>
where
    P: Processor + From<WebState>,
{
    let span = tracing::info_span!(
        "api.dispatch",
        method = ?operation,
        transport = ?context.transport,
        connection_id = context.connection_id.as_ref().map(|id| id.0.as_str()),
    );

    async move {
        if dispatcher_migration_state(operation) != ApiDispatcherMigrationState::Dispatched {
            return Err(ApiError::NotImplemented {
                capability: format!("{operation:?}"),
            });
        }

        if let Some(reason) = experimental_reason(operation) {
            if !experimental_apis_enabled() {
                return Err(ApiError::Experimental(reason.to_string()));
            }
        }

        dispatch_processor(P::from(state), params).await
    }
    .instrument(span)
    .await
}

fn experimental_reason(operation: ApiMethod) -> Option<&'static str> {
    crate::api_operation_registry::api_operation_registry().experimental_reason(operation)
}

fn experimental_apis_enabled() -> bool {
    std::env::var("ALLTHECODES_ENABLE_EXPERIMENTAL_API")
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "on"))
}

fn map_session_list(response: handlers::SessionListResponse) -> v1::SessionListResponse {
    v1::SessionListResponse {
        sessions: response
            .sessions
            .into_iter()
            .map(|session| v1::SessionSummary {
                id: session.session_id,
                title: Some(session.title),
                archived: false,
                chat_mode_override: session.chat_mode_override,
                effective_chat_mode: session.effective_chat_mode,
            })
            .collect(),
    }
}

fn map_session_detail(response: handlers::SessionDetailResponse) -> v1::SessionDetailResponse {
    v1::SessionDetailResponse {
        session: v1::SessionSummary {
            id: response.session_id,
            title: Some(response.title),
            archived: false,
            chat_mode_override: response.chat_mode_override,
            effective_chat_mode: response.effective_chat_mode,
        },
    }
}

fn map_session_mode(response: handlers::SessionModeResponse) -> v1::SessionModePatchResponse {
    v1::SessionModePatchResponse {
        session_id: response.session_id,
        workspace_key: response.workspace_key,
        default_chat_mode: response.default_chat_mode,
        chat_mode_override: response.chat_mode_override,
        effective_chat_mode: response.effective_chat_mode,
    }
}

fn response_to_value<T: Serialize>(response: T) -> Result<serde_json::Value, ApiError> {
    serde_json::to_value(response).map_err(|error| ApiError::Internal {
        message: format!("Failed to serialize dispatcher response: {error}"),
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    use crate::api_operation_registry::DISPATCHED_OPERATIONS;
    use allthecodes_engine::lifecycle::QueryEngine;
    use allthecodes_engine::types::config::QueryEngineConfig;
    use allthecodes_protocol::{DirectTransport, Transport, API_METADATA};
    use axum::http::StatusCode;
    use tempfile::TempDir;

    use super::*;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set_path(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn temp_home() -> (TempDir, EnvGuard) {
        let temp = tempfile::tempdir().expect("tempdir");
        let guard = EnvGuard::set_path("ALLTHECODES_HOME", temp.path());
        (temp, guard)
    }

    fn make_web_state() -> WebState {
        make_web_state_with_cwd(Path::new("."))
    }

    fn make_web_state_with_cwd(cwd: &Path) -> WebState {
        let engine = Arc::new(QueryEngine::new(QueryEngineConfig {
            cwd: cwd.to_string_lossy().to_string(),
            tools: vec![],
            custom_system_prompt: None,
            append_system_prompt: None,
            user_specified_model: None,
            fallback_model: None,
            max_turns: None,
            max_budget_usd: None,
            task_budget: None,
            verbose: false,
            initial_messages: None,
            commands: vec![],
            thinking_config: None,
            json_schema: None,
            replay_user_messages: false,
            persist_session: false,
            resolved_model: None,
            auto_save_session: false,
            agent_context: None,
        }));
        WebState::new(engine, Arc::new(AtomicBool::new(false)))
    }

    #[tokio::test]
    async fn dispatcher_handles_capabilities_request() {
        let response = dispatch(
            make_web_state(),
            ApiRequestContext::direct(),
            ClientRequest::Capabilities(NoParams {}),
        )
        .await
        .expect("capabilities should dispatch");

        match response {
            ClientResponse::Capabilities(body) => {
                assert_eq!(body.capabilities.get("sessions"), Some(&true));
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn dispatcher_handles_health_request() {
        let (_home, _guard) = temp_home();
        let response = dispatch(
            make_web_state(),
            ApiRequestContext::direct(),
            ClientRequest::Health(NoParams {}),
        )
        .await
        .expect("health should dispatch");

        match response {
            ClientResponse::Health(body) => {
                assert_eq!(body.status, "ok");
                assert_eq!(body.db, "connected");
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[tokio::test]
    async fn direct_transport_uses_api_dispatcher() {
        let transport = DirectTransport::new(ApiDispatcher::new(make_web_state()));
        let response = transport
            .send_request(ClientRequest::Capabilities(NoParams {}))
            .await
            .expect("direct dispatcher request should succeed");

        assert!(matches!(response, ClientResponse::Capabilities(_)));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn dispatcher_handles_plugin_disable_request() {
        let (_home, _guard) = temp_home();
        allthecodes_plugins::clear_plugins();
        let plugin_source = tempfile::tempdir().expect("plugin source");
        std::fs::write(
            plugin_source.path().join("plugin.json"),
            r#"{
                "name": "dispatcher-plugin",
                "display_name": "Dispatcher Plugin",
                "version": "1.0.0",
                "description": "Dispatcher test plugin"
            }"#,
        )
        .expect("plugin manifest");

        let state = make_web_state();
        dispatch(
            state.clone(),
            ApiRequestContext::direct(),
            ClientRequest::PluginsInstall(v1::plugins::PluginInstallRequest::Legacy(
                v1::plugins::PluginLegacyInstallRequest {
                    source: plugin_source.path().to_string_lossy().to_string(),
                    scope: Some("user".to_string()),
                },
            )),
        )
        .await
        .expect("plugin install should dispatch");

        let response = dispatch(
            state,
            ApiRequestContext::direct(),
            ClientRequest::PluginsDisable(v1::plugins::PluginIdRequest {
                id: "dispatcher-plugin@local".to_string(),
            }),
        )
        .await
        .expect("plugin disable should dispatch");

        match response {
            ClientResponse::PluginsDisable(body) => {
                assert_eq!(body.status, "disabled");
                assert_eq!(body.plugin["id"], "dispatcher-plugin@local");
            }
            other => panic!("unexpected response: {other:?}"),
        }
        allthecodes_plugins::clear_plugins();
    }

    #[test]
    fn migration_tracker_marks_dispatched_operations() {
        assert_eq!(DISPATCHED_OPERATIONS.len(), 51);

        for operation in DISPATCHED_OPERATIONS {
            assert_eq!(
                dispatcher_migration_state(*operation),
                ApiDispatcherMigrationState::Dispatched,
                "{operation:?} should enter ApiDispatcher"
            );
        }
    }

    #[test]
    fn api_operation_registry_covers_protocol_metadata_and_dispatcher_state() {
        let registry = crate::api_operation_registry::api_operation_registry();

        assert_eq!(registry.len(), API_METADATA.len());
        for metadata in API_METADATA {
            assert!(
                registry.get(metadata.endpoint.operation).is_some(),
                "missing registry entry for {:?}",
                metadata.endpoint.operation
            );
        }

        let capabilities = registry.get(ApiMethod::Capabilities).unwrap();
        assert_eq!(
            capabilities.migration_state,
            ApiDispatcherMigrationState::Dispatched
        );
        assert_eq!(
            capabilities.transport_kind,
            ApiTransportKind::JsonRpcWebSocket
        );

        let terminal_ws = registry.get(ApiMethod::TerminalSessionWs).unwrap();
        assert_eq!(
            terminal_ws.migration_state,
            ApiDispatcherMigrationState::DedicatedTransport
        );
        assert_eq!(
            terminal_ws.transport_kind,
            ApiTransportKind::DedicatedWebSocket
        );

        let ipc_ws = registry.get(ApiMethod::IpcWs).unwrap();
        assert_eq!(
            ipc_ws.migration_state,
            ApiDispatcherMigrationState::LegacyIpcBridge
        );
        assert_eq!(ipc_ws.transport_kind, ApiTransportKind::IpcBridge);
    }

    #[test]
    fn migration_tracker_keeps_dedicated_transports_out() {
        for operation in [
            ApiMethod::TerminalProfiles,
            ApiMethod::TerminalSessionsList,
            ApiMethod::TerminalSessionsCreate,
            ApiMethod::TerminalSessionDetail,
            ApiMethod::TerminalSessionOutput,
            ApiMethod::TerminalSessionDelete,
            ApiMethod::TerminalSessionWs,
            ApiMethod::TuiWs,
            ApiMethod::FilesDownload,
            ApiMethod::FilesMedia,
        ] {
            assert_eq!(
                dispatcher_migration_state(operation),
                ApiDispatcherMigrationState::DedicatedTransport
            );
        }

        assert_eq!(
            dispatcher_migration_state(ApiMethod::IpcWs),
            ApiDispatcherMigrationState::LegacyIpcBridge
        );
    }

    #[tokio::test]
    async fn rest_processor_response_uses_dispatcher_lifecycle() {
        let response = rest_processor_response::<handlers::CapabilitiesProcessor>(
            make_web_state(),
            ApiMethod::Capabilities,
            NoParams {},
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn rest_processor_rejects_untracked_operations() {
        let result = dispatch_rest_processor::<handlers::CapabilitiesProcessor>(
            make_web_state(),
            ApiMethod::State,
            NoParams {},
        )
        .await;

        assert!(matches!(result, Err(ApiError::NotImplemented { .. })));
    }

    #[tokio::test]
    async fn dispatcher_routes_session_requests_through_processors() {
        let error = dispatch(
            make_web_state(),
            ApiRequestContext::direct(),
            ClientRequest::SessionDetail(v1::SessionDetailParams {
                id: "missing-session-for-dispatcher-test".to_string(),
            }),
        )
        .await
        .expect_err("missing session should surface as protocol not_found");

        assert!(matches!(
            error,
            ApiError::NotFound {
                entity: "session",
                ..
            }
        ));
    }

    #[tokio::test]
    async fn dispatcher_handles_session_create_request() {
        let response = dispatch(
            make_web_state(),
            ApiRequestContext::direct(),
            ClientRequest::SessionCreate(v1::SessionCreateParams {
                title: None,
                workspace_key: None,
                cwd: None,
            }),
        )
        .await
        .expect("session create should dispatch");

        match response {
            ClientResponse::SessionCreate(body) => assert!(!body.id.is_empty()),
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatcher_handles_files_tree_request() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(project.path().join("hello.txt"), b"hello").expect("seed file");

        let response = dispatch(
            make_web_state_with_cwd(project.path()),
            ApiRequestContext::direct(),
            ClientRequest::FilesTree(v1::files::FileTreeQuery {
                path: None,
                profile_id: None,
            }),
        )
        .await
        .expect("files tree should dispatch");

        match response {
            ClientResponse::FilesTree(body) => {
                assert!(body.entries.iter().any(|entry| entry.name == "hello.txt"));
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[tokio::test]
    async fn unsupported_request_returns_protocol_error() {
        let error = dispatch(
            make_web_state(),
            ApiRequestContext::direct(),
            ClientRequest::TerminalSessionsCreate(v1::terminal::TerminalCreateRequest {
                profile: "shell".to_string(),
                client_request_id: None,
                cwd: None,
                label: None,
                command: None,
                session_id: None,
                persist: None,
                initial_size: None,
            }),
        )
        .await
        .expect_err("terminal transport must remain out of dispatcher");

        assert!(matches!(error, ApiError::NotImplemented { .. }));
    }
}
